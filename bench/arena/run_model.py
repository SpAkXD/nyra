#!/usr/bin/env python3
"""Nyra Arena: add a model. Runs the web A/B harness (bench/web/) for ONE new model on the briefs of the latest
held-out round, with the same prompts, checklist, repair rule and report, then rebuilds the leaderboard.

    python bench/arena/run_model.py --model claude-opus-6 --provider anthropic --price 5,25 --name "Claude Opus 6"
    python bench/arena/run_model.py --model openai/gpt-6-sol --provider openrouter --vendor OpenAI --cap 10
    python bench/arena/run_model.py --model ... --dry-run          # print the plan, call nothing

Steps (each resumable; a second call continues where the first stopped):
  1. bench/web/run_ab.py   first answers + at most one Nyra repair turn   -> bench/web/results/<date>-<model>/
  2. bench/web/evaluate.py the functional checklist and perf in headless Brave/Chrome (BROWSER=... elsewhere)
  3. bench/web/report.py   REPORT.md, runs.csv, summary.json
  4. bench/arena/build.py  arena.json (and --site <dir>/data.js when given)

Rules that keep the result comparable with the published rounds:
  * the briefs, prompts (bench/web/prompts/<brief>/) and checklists (bench/web/checks/) of --rounds are used unchanged;
  * both languages are run (the model's own HTML score is the baseline column);
  * --runs per brief and language (default 4: 2 briefs x 4 = 8 Nyra answers, the minimum to rank; fewer = provisional);
  * a hard spend cap (--cap, US dollars) stops new calls before it could be passed.
Keys come from the environment only: ANTHROPIC_API_KEY (anthropic provider, and for counting code tokens on the
claude-sonnet-5-5 tokenizer: count_tokens is free), OPENROUTER_API_KEY (openrouter provider). Needs a release build of
the compiler (cargo build --release), Node.js 22+ and a Chromium browser for the checklist.
"""
from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import pathlib
import re
import subprocess
import sys
import time

HERE = pathlib.Path(__file__).resolve().parent
BENCH = HERE.parent
ROOT = BENCH.parent
WEB = BENCH / "web"
RESULTS = WEB / "results"
sys.path.insert(0, str(BENCH))
sys.path.insert(0, str(WEB))
sys.path.insert(0, str(HERE))


def slug(model: str) -> str:
    return re.sub(r"[^a-z0-9.]+", "-", model.lower()).strip("-")


def pick_round(spec: str) -> pathlib.Path:
    """The results folder whose briefs are reused: 'latest' = the newest held-out round that is not itself an
    arena add-on (it has no arena.json), or a folder name under bench/web/results/."""
    if spec != "latest":
        d = RESULTS / spec
        if not (d / "summary.json").exists():
            sys.exit(f"no round {spec} (expected {d / 'summary.json'})")
        return d
    cands = []
    for d in RESULTS.iterdir():
        f = d / "summary.json"
        if d.is_dir() and f.exists() and not (d / "arena.json").exists():
            s = json.loads(f.read_text(encoding="utf-8"))
            if s.get("held_out"):
                cands.append((s.get("date", d.name), d))
    if not cands:
        sys.exit("no held-out round found in bench/web/results/")
    return max(cands)[1]


def openrouter_price(model: str):
    import models as orm  # bench/models.py
    listing = orm.fetch_models()
    entry = orm.lookup({m["id"]: m for m in listing}, model)
    if entry is None:
        sys.exit(f"{model} is not on OpenRouter (python bench/models.py <words> searches the list)")
    p = orm.price_per_token(entry)
    if p is None:
        sys.exit(f"{model} has no plain per-token price on OpenRouter; pass --price IN,OUT (USD per million tokens)")
    return p[0] * 1e6, p[1] * 1e6, entry.get("name")


class OpenRouterClient:
    """The run_ab.Client interface (ask, count) over OpenRouter's chat completions. Costs are what OpenRouter
    reports (usage.cost). Code tokens are counted on run_ab.TOKENIZER through Anthropic's free count_tokens when
    ANTHROPIC_API_KEY is set, so that the Nyra/HTML token ratio is on the same tokenizer as the other rounds."""

    def __init__(self, model: str, effort, max_tokens: int, anthropic_client=None):
        import providers
        import run_ab
        self.providers, self.run_ab = providers, run_ab
        self.p = providers.OpenRouterProvider(model, max_tokens=max_tokens, effort=effort, count_tokens=False)
        self.p.ensure_ready()
        self.effort, self.max_tokens = effort, max_tokens
        self.anth = None
        if os.environ.get("ANTHROPIC_API_KEY"):
            try:
                self.anth = anthropic_client()
            except BaseException as exc:  # no SDK: code tokens stay empty
                print(f"code tokens will not be counted: {exc}", file=sys.stderr)
                self.anth = None
        else:
            print("ANTHROPIC_API_KEY not set: code tokens (and the Nyra/HTML token ratio) will be empty", file=sys.stderr)

    @staticmethod
    def _plain(messages: list) -> list:
        out = []
        for m in messages:
            c = m["content"]
            if isinstance(c, list):  # keep text parts (and cache_control, which OpenRouter forwards where supported)
                c = [{k: v for k, v in part.items() if k in ("type", "text", "cache_control")}
                     for part in c if part.get("type") == "text"]
            out.append({"role": m["role"], "content": c})
        return out

    def ask(self, model: str, messages: list) -> dict:
        body = {"model": model, "max_tokens": self.max_tokens, "messages": self._plain(messages), "usage": {"include": True}}
        if self.effort:
            body["reasoning"] = {"effort": self.effort}
        t0 = time.monotonic()
        parsed, _ = self.p._post(body)
        secs = time.monotonic() - t0
        u = self.providers.usage_from_openrouter(parsed.get("usage"))
        raw_u = parsed.get("usage") or {}
        read = int(((raw_u.get("prompt_tokens_details") or {}).get("cached_tokens")) or 0)
        choice = parsed["choices"][0]
        text = self.providers._text_of((choice.get("message") or {}).get("content"))
        return {"text": text, "blocks": [], "stop_reason": choice.get("finish_reason"), "model_served": parsed.get("model"),
                "upstream": self.providers._upstream_provider(parsed), "request_id": parsed.get("id"),
                "seconds": round(secs, 1), "input_tokens": int(u.input_tokens or 0),
                "input_fresh": int(u.input_tokens or 0) - read, "cache_write": 0, "cache_read": read,
                "output_tokens": int(u.output_tokens or 0), "reasoning_tokens": u.reasoning_tokens,
                "cost_usd": round(u.cost or 0.0, 6)}

    def count(self, model: str, text: str):
        if self.anth is None or model != self.run_ab.TOKENIZER:
            return None  # the model's own tokenizer has no free counter here; thinking estimates stay empty
        return self.anth.count(model, text)


METHOD = """## Method

An arena add-on run: one model, the briefs of `{base}` ({briefs}), made by `bench/arena/run_model.py` on {date}.
Everything else is the method of `bench/web/results/{base}/` (read its REPORT.md): the same prompts
(`bench/web/prompts/<brief>/`), the same functional checklist (`bench/web/checks/`, `check.js`), one repair turn with
the compiler's message for Nyra only, failed builds score 0, desktop 1280x900.

**Model.** `{model}` through {provider_desc}, effort `{effort}`, max_tokens {max_tokens}. {runs} runs per brief and
language. Spend cap ${cap:.2f}. Compiler: {nyra}.

**Caveat.** These briefs were published with round `{base}`, so a model trained after that date may have seen them.
"""


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description="Add one model to the Nyra Arena (web A/B on the latest held-out briefs).")
    ap.add_argument("--model", required=True, help="model id: an Anthropic id, or an OpenRouter id like openai/gpt-6-sol")
    ap.add_argument("--provider", choices=["anthropic", "openrouter"], default="anthropic", help="API to call")
    ap.add_argument("--rounds", default="latest", help="whose briefs to reuse: 'latest' or a results folder name")
    ap.add_argument("--runs", type=int, default=4, help="runs per brief and language (default 4 = 8 Nyra answers)")
    ap.add_argument("--cap", type=float, default=10.0, help="hard spend cap in US dollars (default 10)")
    ap.add_argument("--parallel", type=int, default=4)
    ap.add_argument("--effort", default="medium", help="effort / reasoning effort, or 'none' for the model's default")
    ap.add_argument("--max-tokens", type=int, default=64000)
    ap.add_argument("--price", default="", help="IN,OUT USD per million tokens (needed for an Anthropic id the harness "
                                                "does not know; OpenRouter prices are read from its model list)")
    ap.add_argument("--name", default="", help="display name (default: the id, or OpenRouter's name)")
    ap.add_argument("--vendor", default="", help="who makes the model, shown as its provider (default: from the id)")
    ap.add_argument("--license", default="Proprietary", help="license label shown on the leaderboard")
    ap.add_argument("--out", default="", help="results folder (default bench/web/results/<today>-<model>)")
    ap.add_argument("--site", default="", help="also write the leaderboard's data.js into this folder")
    ap.add_argument("--skip-generate", action="store_true", help="only evaluate, report and rebuild (resume)")
    ap.add_argument("--dry-run", action="store_true", help="print the plan and stop")
    a = ap.parse_args(argv)

    base = pick_round(a.rounds)
    summary = json.loads((base / "summary.json").read_text(encoding="utf-8"))
    briefs = summary["briefs"]
    out = pathlib.Path(a.out).resolve() if a.out else RESULTS / f"{dt.date.today().isoformat()}-{slug(a.model)}"
    effort = None if a.effort.lower() in ("", "none", "default") else a.effort

    import providers
    import run_ab
    price = None
    or_name = None
    if a.price:
        pin, pout = (float(x) for x in a.price.split(","))
        price = (pin, pout)
    elif a.provider == "anthropic":
        p = providers.ANTHROPIC_PRICES.get(a.model)
        if p is None:
            sys.exit(f"no price for {a.model} in bench/providers.py: pass --price IN,OUT")
        price = (p.input, p.output)
    elif not a.dry_run:
        pin, pout, or_name = openrouter_price(a.model)
        price = (pin, pout)

    name = a.name or or_name or a.model
    vendor = a.vendor or ("Anthropic" if a.provider == "anthropic" else a.model.split("/")[0].replace("-", " ").title())
    plan = {"model": a.model, "provider": a.provider, "name": name, "vendor": vendor, "base_round": base.name,
            "briefs": briefs, "runs_per_brief_and_language": a.runs, "nyra_answers": a.runs * len(briefs),
            "cap_usd": a.cap, "effort": effort, "max_tokens": a.max_tokens, "price_per_mtok": price, "out": str(out)}
    print(json.dumps(plan, indent=1))
    if a.runs * len(briefs) < 8:
        print("note: fewer than 8 Nyra answers: the model will be listed as provisional", file=sys.stderr)
    if a.dry_run:
        return 0

    out.mkdir(parents=True, exist_ok=True)
    nyra_ver = subprocess.run(["git", "rev-parse", "--short", "HEAD"], cwd=ROOT, capture_output=True, text=True).stdout.strip()
    branch = subprocess.run(["git", "rev-parse", "--abbrev-ref", "HEAD"], cwd=ROOT, capture_output=True, text=True).stdout.strip()
    side = {
        "label": f"{name} · {', '.join(briefs)} briefs",
        "base_round": base.name, "held_out": True, "added_by": "bench/arena/run_model.py",
        "nyra": f"{branch} @ {nyra_ver}",
        "note": f"Arena add-on on the briefs of {base.name}; the briefs were public from that date.",
        "models": {a.model: {"name": name, "short": name, "provider": vendor, "license": a.license,
                             "price": {"in": price[0], "out": price[1]} if price else None,
                             "api": a.provider, "effort": effort}},
    }
    (out / "arena.json").write_text(json.dumps(side, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")

    # ---- 1. generate (run_ab.py, patched for one model)
    if not a.skip_generate:
        run_ab.MODELS = [a.model]
        run_ab.BRIEFS = briefs
        run_ab.MAX_TOKENS[a.model] = a.max_tokens
        if effort:
            run_ab.EFFORT[a.model] = effort
        else:
            run_ab.EFFORT.pop(a.model, None)
        if a.model not in run_ab.ANTHROPIC_PRICES:  # used for the spend reservations (and the Anthropic cost)
            run_ab.ANTHROPIC_PRICES[a.model] = providers.Price(price[0], price[1])
        if a.provider == "openrouter":
            anthropic_client = run_ab.Client  # kept for counting code tokens on the shared tokenizer
            run_ab.Client = lambda: OpenRouterClient(a.model, effort, a.max_tokens, anthropic_client)  # noqa: E731
        sys.argv = ["run_ab.py", "--out", str(out), "--runs", str(a.runs), "--cap", str(a.cap),
                    "--parallel", str(a.parallel), "--models", a.model, "--briefs", ",".join(briefs)]
        run_ab.main()

    # ---- 2. evaluate
    r = subprocess.run([sys.executable, str(WEB / "evaluate.py"), "--out", str(out)], cwd=ROOT)
    if r.returncode:
        return r.returncode

    # ---- 3. report
    meth = out / "method.md"
    if not meth.exists():
        meth.write_text(METHOD.format(base=base.name, briefs=", ".join(briefs), date=dt.date.today().isoformat(),
                                      model=a.model, provider_desc=("the Anthropic API" if a.provider == "anthropic" else "OpenRouter"),
                                      effort=effort or "default", max_tokens=a.max_tokens, runs=a.runs, cap=a.cap,
                                      nyra=side["nyra"]), encoding="utf-8")
    import report
    report.MODEL_NAMES[a.model] = name
    sys.argv = ["report.py", "--out", str(out), "--cap", str(a.cap)]
    report.main()

    # ---- 4. leaderboard
    import build
    args = ["--site", a.site] if a.site else []
    return build.main(args)


if __name__ == "__main__":
    sys.exit(main())
