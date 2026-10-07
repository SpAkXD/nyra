#!/usr/bin/env python3
"""Find and check OpenRouter model ids. The list is public: no API key is needed (or sent).

    python bench/models.py                      # the default models of bench/models.json, with current prices
    python bench/models.py claude opus          # models whose id or name contains every word
    python bench/models.py --check              # do all default ids still exist? (exit status 1 if not)
    python bench/models.py --check --ids a/b,c/d    # check these ids instead
    python bench/models.py --json gemini        # the raw entries, for scripts

Model ids look like `anthropic/claude-opus-5.5`. Ids ending in `:free` or `:batch` are variants with different
limits or prices; `~vendor/...-latest` ids are aliases that move to a newer model over time, so pin a concrete id
for a benchmark. The default list is bench/models.json (edit it freely): it only holds ids that were checked
against the live list, and the `verified` date says when.

The library part (fetch_models, default_model_ids, missing_ids, price_per_token) is used by run.py to refuse a
typo before any money is spent.
"""

from __future__ import annotations

import argparse
import difflib
import http.client
import json
import sys
from pathlib import Path
from typing import Optional

BENCH_DIR = Path(__file__).resolve().parent
if str(BENCH_DIR) not in sys.path:
    sys.path.insert(0, str(BENCH_DIR))

import providers  # noqa: E402

DEFAULT_MODELS_FILE = BENCH_DIR / "models.json"


class ModelsError(Exception):
    """The model list could not be fetched or the config file is unusable."""


def fetch_models(base_url: str = providers.OPENROUTER_BASE_URL, transport: Optional[providers.Transport] = None,
                 timeout: float = 30.0) -> list:
    """GET {base_url}/models: every model OpenRouter serves. Public, so no Authorization header is sent."""
    url = providers.check_base_url(base_url) + "/models"
    headers = {"Accept": "application/json", "User-Agent": providers._USER_AGENT}
    try:
        status, _, raw = (transport or providers.urllib_transport)("GET", url, headers, None, timeout)
    except (OSError, http.client.HTTPException) as exc:
        raise ModelsError(f"cannot reach {url}: {type(exc).__name__}: {exc}") from None
    parsed = providers.loads_json(raw)
    if status != 200 or not isinstance(parsed, dict) or not isinstance(parsed.get("data"), list):
        raise ModelsError(f"{url} answered HTTP {status} without a model list")
    return [m for m in parsed["data"] if isinstance(m, dict) and isinstance(m.get("id"), str)]


def load_default_models(path: Path = DEFAULT_MODELS_FILE) -> dict:
    """bench/models.json: {"verified": date, "models": [{"id": ..., "note": ...}, ...]}."""
    try:
        data = json.loads(Path(path).read_text(encoding="utf-8"))
    except (OSError, ValueError) as exc:
        raise ModelsError(f"cannot read {Path(path).name}: {exc}") from None
    entries = data.get("models") if isinstance(data, dict) else None
    if not isinstance(entries, list) or not entries:
        raise ModelsError(f"{Path(path).name} needs a non-empty \"models\" list")
    ids = []
    for entry in entries:
        mid = entry.get("id") if isinstance(entry, dict) else entry
        if not isinstance(mid, str) or not mid.strip() or mid != mid.strip() or " " in mid:
            raise ModelsError(f"{Path(path).name}: bad model entry {entry!r}")
        ids.append(mid)
    if len(set(ids)) != len(ids):
        raise ModelsError(f"{Path(path).name}: duplicate model ids")
    return data


def default_model_ids(path: Path = DEFAULT_MODELS_FILE) -> list:
    data = load_default_models(path)
    return [e["id"] if isinstance(e, dict) else e for e in data["models"]]


def lookup(by_id: dict, model_id: str) -> Optional[dict]:
    """The listing entry of a model id. `vendor/model:variant` (:online, :nitro, :floor, ...) is the same model with
    different routing and is not listed on its own, so it falls back to `vendor/model`."""
    entry = by_id.get(model_id)
    if entry is None and ":" in model_id:
        entry = by_id.get(model_id.rsplit(":", 1)[0])
    return entry


def missing_ids(listing: list, ids: list) -> list:
    """The ids that are not in the listing, each with up to three similar ids: [(id, [suggestions])]."""
    by_id = {m["id"]: m for m in listing}
    out = []
    for mid in ids:
        if lookup(by_id, mid) is None:
            near = difflib.get_close_matches(mid, sorted(by_id), n=3, cutoff=0.6)
            out.append((mid, near))
    return out


def search(listing: list, words: list) -> list:
    """Models whose id or name contains every word (case-insensitive), newest first."""
    words = [w.lower() for w in words]
    hits = [m for m in listing if all(w in f"{m['id']} {m.get('name', '')}".lower() for w in words)]
    return sorted(hits, key=lambda m: -(m.get("created") or 0))


def price_per_token(model: dict) -> Optional[tuple]:
    """(input, output) dollars per token from a listing entry, or None when the price is not a plain number."""
    pricing = model.get("pricing")
    if not isinstance(pricing, dict):
        return None
    try:
        prompt, completion = float(pricing["prompt"]), float(pricing["completion"])
    except (KeyError, TypeError, ValueError):
        return None
    return (prompt, completion) if prompt >= 0 and completion >= 0 else None  # "-1": routers like openrouter/auto


def thinking_note(model: dict) -> str:
    r = model.get("reasoning")
    if not isinstance(r, dict):
        return "no thinking"
    if r.get("mandatory"):
        return "thinks always"
    return "thinks by default" if r.get("default_enabled") else "thinking optional"


def describe(model: dict) -> str:
    price = price_per_token(model)
    cost = f"${price[0] * 1e6:.2f} in / ${price[1] * 1e6:.2f} out per M tokens" if price else "price n/a"
    context = model.get("context_length")
    context = context if isinstance(context, int) and not isinstance(context, bool) else 0
    return f"{model['id']:<42} {cost:<40} {context:>9,} ctx  {thinking_note(model)}"


def main(argv=None) -> int:
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8", errors="replace")
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0], formatter_class=argparse.RawDescriptionHelpFormatter,
                                epilog=__doc__.split("\n\n", 1)[1])
    p.add_argument("words", nargs="*", help="show the models whose id or name contains all of these words")
    p.add_argument("--check", action="store_true", help="verify that the default ids (or --ids) exist; exit 1 if not")
    p.add_argument("--ids", help="comma-separated ids for --check (default: the list in bench/models.json)")
    p.add_argument("--file", default=str(DEFAULT_MODELS_FILE), help="the default list (default: bench/models.json)")
    p.add_argument("--json", action="store_true", help="print the matching entries as JSON")
    p.add_argument("--limit", type=int, default=40, help="most entries to print for a search (default: 40)")
    args = p.parse_args(argv)
    try:
        listing = fetch_models()
        if args.check:
            ids = [i.strip() for i in args.ids.split(",") if i.strip()] if args.ids else default_model_ids(Path(args.file))
            bad = missing_ids(listing, ids)
            for mid, near in bad:
                print(f"MISSING {mid}" + (f"  (similar: {', '.join(near)})" if near else ""))
            print(f"{len(ids) - len(bad)} of {len(ids)} id(s) exist on OpenRouter ({len(listing)} models listed)")
            return 1 if bad else 0
        if args.words:
            hits = search(listing, args.words)
        else:
            by_id = {m["id"]: m for m in listing}
            data = load_default_models(Path(args.file))
            hits = [by_id[i] for i in default_model_ids(Path(args.file)) if i in by_id]
            print(f"default models from {Path(args.file).name} (verified {data.get('verified', '?')}):")
    except ModelsError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    if args.json:
        print(json.dumps(hits[:args.limit], indent=2, ensure_ascii=False))
        return 0
    for m in hits[:args.limit]:
        print(describe(m))
    if len(hits) > args.limit:
        print(f"... {len(hits) - args.limit} more (narrow the search, or raise --limit)")
    if not hits:
        print("no model matches")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
