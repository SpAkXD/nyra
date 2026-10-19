"""The web A/B test, step 1 of 3: ask the models for the sites (plain one-file HTML vs one-file Nyra `use web`).

    python bench/web/run_ab.py --out bench/web/results/2026-10-19 [--runs 4] [--cap 20] [--parallel 6]

Needs ANTHROPIC_API_KEY (read from the environment only) and a built compiler (target/release/nyra).

For every brief x model x language x run: one first answer to the unchanged prompt from make_prompts.py (the whole
prompt is one user message, cached with prompt caching: the brief and, for Nyra, the two cards). A Nyra file that
does not build gets at most ONE repair turn with the compiler's message (like a developer pasting the error back);
HTML has no compiler, so it never gets a repair turn (the report says so). Every model file is kept, broken or not:

    <out>/raw/<brief>/<model>/<lang>-<i>.html|.nyra      first answer (code block)
    <out>/raw/<brief>/<model>/nyra-<i>-fix.nyra          the repaired file, if there was a repair turn
    <out>/runs/<brief>/<model>/<lang>-<i>.json           tokens, cost, time, build results
    <out>/sites/<brief>/<model>/<lang>-<i>/              what is served and tested (index.html, or Nyra's dist/)

The runner is resumable (a run with a JSON file is not asked again) and stops starting new calls when the next one
could pass the spend cap (--cap, US dollars, the provider's own cost accounting from bench/providers.py). Jobs run
in run-major order (run 1 of every brief/model/language first), so a run cut short by the cap stays balanced.
"""
from __future__ import annotations

import argparse
import concurrent.futures as cf
import datetime as dt
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import threading
import time

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent.parent
sys.path.insert(0, str(HERE.parent))
from providers import ANTHROPIC_PRICES, anthropic_cost  # noqa: E402

MODELS = ["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-4-5-20251001"]
BRIEFS = ["dentist", "gym"]
LANGS = ["html", "nyra"]
EXT = {"html": "html", "nyra": "nyra"}
# medium effort where the API has the setting (output_config.effort, as bench/providers.py sends it). Claude Haiku
# 4.5 rejects `effort`, so it runs with its defaults (no extended thinking), like the existing benchmark does.
EFFORT = {"claude-opus-5-5": "medium", "claude-sonnet-5-5": "medium"}
MAX_TOKENS = {"claude-opus-5-5": 64000, "claude-sonnet-5-5": 64000, "claude-haiku-4-5-20251001": 64000}
# what one call is allowed to cost at most when deciding whether to start it (output well above a full site)
RESERVE_OUT_TOKENS = 45000
TOKENIZER = "claude-sonnet-5-5"  # code tokens of every final file are counted on this one tokenizer

NYRA = ROOT / "target" / "release" / ("nyra.exe" if os.name == "nt" else "nyra")
FENCE = re.compile(r"```[A-Za-z0-9_+-]*[ \t]*\r?\n(.*?)(?:\r?\n)?```", re.S)
OPEN_FENCE = re.compile(r"```[A-Za-z0-9_+-]*[ \t]*\r?\n(.*)\Z", re.S)


def extract_code(text: str) -> str:
    """The largest fenced code block. A reply cut off inside its block keeps what is there; a reply with only a
    closing fence (or none) is the text without the stray fence lines (what a person copying it would paste)."""
    blocks = FENCE.findall(text)
    if blocks:
        return max(blocks, key=len).strip() + "\n"
    m = OPEN_FENCE.search(text)
    code = m.group(1) if m else text
    code = re.sub(r"\A\s*```[A-Za-z0-9_+-]*[ \t]*\r?\n", "", code)
    code = re.sub(r"\r?\n```\s*\Z", "\n", code.rstrip() + "\n")
    return code.strip() + "\n"


class Spend:
    def __init__(self, cap: float):
        self.cap, self.spent, self.reserved, self.lock = cap, 0.0, 0.0, threading.Lock()
        self.stopped = 0

    def reserve(self, amount: float) -> bool:
        with self.lock:
            if self.spent + self.reserved + amount > self.cap:
                self.stopped += 1
                return False
            self.reserved += amount
            return True

    def settle(self, reserved: float, actual: float):
        with self.lock:
            self.reserved -= reserved
            self.spent += actual


class Client:
    def __init__(self):
        import anthropic
        key = os.environ.get("ANTHROPIC_API_KEY")
        if not key:
            sys.exit("ANTHROPIC_API_KEY is not set")
        headers = {}
        if os.environ.get("ANTHROPIC_WORKSPACE_ID"):
            headers["anthropic-workspace-id"] = os.environ["ANTHROPIC_WORKSPACE_ID"].strip()
        self.c = anthropic.Anthropic(api_key=key, max_retries=8, timeout=1800.0, default_headers=headers or None)
        self.baseline = {}
        self.lock = threading.Lock()

    def ask(self, model: str, messages: list) -> dict:
        req = {"model": model, "max_tokens": MAX_TOKENS[model], "messages": messages}
        if model in EFFORT:
            req["output_config"] = {"effort": EFFORT[model]}
        t0 = time.monotonic()
        with self.c.messages.stream(**req) as stream:
            msg = stream.get_final_message()
        secs = time.monotonic() - t0
        u = msg.usage
        fresh, created, read = (int(getattr(u, f, 0) or 0) for f in ("input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens"))
        out = int(u.output_tokens or 0)
        cost = anthropic_cost(ANTHROPIC_PRICES[model], fresh + created + read, out, created, read)
        text = "".join(b.text for b in msg.content if b.type == "text")
        blocks = [b.model_dump(exclude_none=True) for b in msg.content if b.type in ("text", "thinking", "redacted_thinking")]
        return {"text": text, "blocks": blocks, "stop_reason": msg.stop_reason, "model_served": msg.model,
                "request_id": getattr(msg, "_request_id", None), "seconds": round(secs, 1),
                "input_tokens": fresh + created + read, "input_fresh": fresh, "cache_write": created, "cache_read": read,
                "output_tokens": out, "cost_usd": round(cost, 6)}

    def count(self, model: str, text: str):
        """Tokens of `text` alone on `model`'s tokenizer (count_tokens of the text minus that of "x", plus 1)."""
        if not text.strip():
            return 0
        try:
            with self.lock:
                if model not in self.baseline:
                    self.baseline[model] = self.c.messages.count_tokens(model=model, messages=[{"role": "user", "content": "x"}]).input_tokens - 1
            n = self.c.messages.count_tokens(model=model, messages=[{"role": "user", "content": text}]).input_tokens
            return max(0, n - self.baseline[model])
        except Exception as exc:  # counting is free but rate limited; a miss is recorded as None
            print(f"count_tokens failed: {type(exc).__name__}: {str(exc)[:120]}", file=sys.stderr)
            return None


def build_nyra(code: str, site: pathlib.Path) -> tuple[bool, str]:
    """`nyra build site.nyra -o dist` in a fresh folder; the served site is that dist/."""
    if site.exists():
        shutil.rmtree(site)
    work = site.parent / (site.name + ".src")
    if work.exists():
        shutil.rmtree(work)
    work.mkdir(parents=True)
    (work / "site.nyra").write_text(code, encoding="utf-8", newline="\n")
    b = subprocess.run([str(NYRA), "build", "site.nyra", "-o", "dist"], cwd=work, capture_output=True,
                       encoding="utf-8", errors="replace", timeout=300)
    msg = ((b.stderr or "") + ("\n" if b.stderr and b.stdout else "") + (b.stdout or "")).strip()
    ok = b.returncode == 0 and (work / "dist" / "index.html").is_file()
    if ok:
        shutil.copytree(work / "dist", site)
    shutil.rmtree(work, ignore_errors=True)
    return ok, msg


def first_error(msg: str) -> str:
    """The first error of a compiler message: its first line that says error, or the first line."""
    lines = [l for l in msg.splitlines() if l.strip()]
    for i, line in enumerate(lines):
        if re.search(r"\berror\b|E\d{4}", line):
            return "\n".join(lines[i:i + 3])[:400]
    return "\n".join(lines[:3])[:400]


REPAIR = ("The compiler rejected site.nyra:\n\n```\n{msg}\n```\n\nFix it. Reply with the complete corrected `site.nyra` "
          "in one code block and nothing else.")


def reserve_for(model: str, prompt_tokens: int) -> float:
    p = ANTHROPIC_PRICES[model]
    return (RESERVE_OUT_TOKENS * p.output + 2 * prompt_tokens * p.input * p.cache_write) / 1e6


def run_one(cl: Client, spend: Spend, out: pathlib.Path, prompts: dict, brief: str, model: str, lang: str, i: int):
    tag = f"{brief}/{model}/{lang}-{i}"
    meta_path = out / "runs" / brief / model / f"{lang}-{i}.json"
    if meta_path.exists():
        return json.loads(meta_path.read_text(encoding="utf-8"))
    prompt = prompts[(brief, lang)]
    res = reserve_for(model, len(prompt) // 3)
    if not spend.reserve(res):
        print(f"skip {tag}: the spend cap would be passed", flush=True)
        return {"brief": brief, "model": model, "lang": lang, "run": i, "skipped": "cap"}
    raw = out / "raw" / brief / model
    raw.mkdir(parents=True, exist_ok=True)
    site = out / "sites" / brief / model / f"{lang}-{i}"
    messages = [{"role": "user", "content": [{"type": "text", "text": prompt, "cache_control": {"type": "ephemeral"}}]}]
    try:
        first = cl.ask(model, messages)
    except Exception as exc:
        spend.settle(res, 0.0)
        print(f"FAIL {tag}: {type(exc).__name__}: {str(exc)[:200]}", flush=True)
        return {"brief": brief, "model": model, "lang": lang, "run": i, "error": f"{type(exc).__name__}: {str(exc)[:300]}"}
    spend.settle(res, first["cost_usd"])
    code = extract_code(first["text"])
    (raw / f"{lang}-{i}.{EXT[lang]}").write_text(code, encoding="utf-8", newline="\n")
    (raw / f"{lang}-{i}.reply.txt").write_text(first["text"], encoding="utf-8", newline="\n")
    rec = {"brief": brief, "model": model, "lang": lang, "run": i, "effort": EFFORT.get(model), "max_tokens": MAX_TOKENS[model],
           "first": {k: v for k, v in first.items() if k not in ("text", "blocks")}}
    rec["first"]["visible_tokens"] = cl.count(model, first["text"])
    if rec["first"]["visible_tokens"] is not None:
        rec["first"]["thinking_tokens_est"] = max(0, first["output_tokens"] - rec["first"]["visible_tokens"])
    final_code = code
    if lang == "html":
        if site.exists():
            shutil.rmtree(site)
        site.mkdir(parents=True)
        (site / "index.html").write_text(code, encoding="utf-8", newline="\n")
        rec["builds_first"] = None
    else:
        ok, msg = build_nyra(code, site)
        rec["builds_first"] = ok
        rec["build_message_first"] = msg[-3000:]
        if not ok:
            rec["first_error"] = first_error(msg)
            res2 = reserve_for(model, (len(prompt) + len(first["text"])) // 3)
            if not spend.reserve(res2):
                rec["repair"] = {"skipped": "cap"}
            else:
                fix_msgs = messages + [{"role": "assistant", "content": first["blocks"] or first["text"]},
                                       {"role": "user", "content": REPAIR.format(msg=msg[:4000])}]
                try:
                    try:
                        second = cl.ask(model, fix_msgs)
                    except Exception as exc:  # replayed thinking blocks refused: resend the visible text only
                        if "400" not in str(exc) and "invalid" not in str(exc).lower():
                            raise
                        fix_msgs[1] = {"role": "assistant", "content": first["text"]}
                        second = cl.ask(model, fix_msgs)
                        second["note"] = "thinking blocks not replayed: " + str(exc)[:160]
                    spend.settle(res2, second["cost_usd"])
                    code2 = extract_code(second["text"])
                    (raw / f"nyra-{i}-fix.nyra").write_text(code2, encoding="utf-8", newline="\n")
                    (raw / f"nyra-{i}-fix.reply.txt").write_text(second["text"], encoding="utf-8", newline="\n")
                    ok2, msg2 = build_nyra(code2, site)
                    rec["repair"] = {k: v for k, v in second.items() if k not in ("text", "blocks")}
                    rec["repair"]["visible_tokens"] = cl.count(model, second["text"])
                    if rec["repair"]["visible_tokens"] is not None:
                        rec["repair"]["thinking_tokens_est"] = max(0, second["output_tokens"] - rec["repair"]["visible_tokens"])
                    rec["builds_after_repair"] = ok2
                    rec["build_message_repair"] = msg2[-3000:]
                    if not ok2:
                        rec["repair_error"] = first_error(msg2)
                    final_code = code2
                except Exception as exc:
                    spend.settle(res2, 0.0)
                    rec["repair"] = {"error": f"{type(exc).__name__}: {str(exc)[:300]}"}
        else:
            rec["builds_after_repair"] = True
    if lang == "nyra" and "builds_after_repair" not in rec:
        rec["builds_after_repair"] = False
    rec["site"] = str(site.relative_to(out)).replace("\\", "/") if site.exists() else None
    rec["final_file"] = f"raw/{brief}/{model}/" + (f"nyra-{i}-fix.nyra" if final_code is not code else f"{lang}-{i}.{EXT[lang]}")
    rec["code_tokens"] = cl.count(TOKENIZER, final_code)
    rec["code_bytes"] = len(final_code.encode("utf-8"))
    rec["code_tokens_first"] = rec["code_tokens"] if final_code is code else cl.count(TOKENIZER, code)
    calls = [rec["first"]] + ([rec["repair"]] if isinstance(rec.get("repair"), dict) and "cost_usd" in rec["repair"] else [])
    rec["total"] = {k: sum(c.get(k) or 0 for c in calls) for k in ("input_tokens", "cache_write", "cache_read", "output_tokens", "cost_usd", "seconds")}
    rec["total"]["thinking_tokens_est"] = sum(c.get("thinking_tokens_est") or 0 for c in calls)
    rec["total"]["calls"] = len(calls)
    meta_path.parent.mkdir(parents=True, exist_ok=True)
    meta_path.write_text(json.dumps(rec, indent=1, ensure_ascii=False), encoding="utf-8")
    b = "" if lang == "html" else f" builds {rec['builds_first']}/{rec['builds_after_repair']}"
    print(f"done {tag}: out {first['output_tokens']} tok, {first['seconds']} s, ${rec['total']['cost_usd']:.3f}{b}"
          f" | spent ${spend.spent:.2f}", flush=True)
    return rec


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=str(HERE / "results" / dt.date.today().isoformat()))
    ap.add_argument("--runs", type=int, default=4)
    ap.add_argument("--cap", type=float, default=20.0)
    ap.add_argument("--parallel", type=int, default=6)
    ap.add_argument("--models", default=",".join(MODELS))
    ap.add_argument("--briefs", default=",".join(BRIEFS))
    ap.add_argument("--langs", default=",".join(LANGS))
    a = ap.parse_args()
    out = pathlib.Path(a.out).resolve()
    if not NYRA.is_file():
        sys.exit(f"build the compiler first: {NYRA}")
    models, briefs, langs = a.models.split(","), a.briefs.split(","), a.langs.split(",")
    prompts = {(b, l): (HERE / "prompts" / b / f"prompt_{l}.md").read_text(encoding="utf-8") for b in briefs for l in langs}
    spent_before = 0.0
    for f in (out / "runs").rglob("*.json") if (out / "runs").exists() else []:
        spent_before += json.loads(f.read_text(encoding="utf-8")).get("total", {}).get("cost_usd", 0.0)
    spend = Spend(a.cap)
    spend.spent = spent_before
    print(f"cap ${a.cap:.2f}, already spent in {out.name}: ${spent_before:.2f}", flush=True)
    cl = Client()
    jobs = [(b, m, l, i) for i in range(1, a.runs + 1) for b in briefs for m in models for l in langs]
    with cf.ThreadPoolExecutor(a.parallel) as ex:
        results = list(ex.map(lambda j: run_one(cl, spend, out, prompts, *j), jobs))
    done = [r for r in results if "total" in r]
    meta = {"started_jobs": len(jobs), "done": len(done), "skipped_cap": sum(1 for r in results if r.get("skipped")),
            "errors": [r for r in results if r.get("error")], "spent_usd": round(spend.spent, 4), "cap_usd": a.cap,
            "models": models, "briefs": briefs, "langs": langs, "runs": a.runs, "effort": EFFORT, "max_tokens": MAX_TOKENS,
            "tokenizer_for_code_tokens": TOKENIZER, "nyra": subprocess.run([str(NYRA), "--version"], capture_output=True, text=True).stdout.strip(),
            "finished": dt.datetime.now().isoformat(timespec="seconds")}
    (out / "generation.json").write_text(json.dumps(meta, indent=1), encoding="utf-8")
    print(json.dumps({k: meta[k] for k in ("done", "skipped_cap", "spent_usd")}), flush=True)


if __name__ == "__main__":
    main()
