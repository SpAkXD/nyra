"""The web A/B test, step 3 of 3: tables, REPORT.md and a compact summary.json from a results folder.

    python bench/web/report.py --out bench/web/results/2026-10-19

Reads <out>/runs, <out>/eval, <out>/generation.json and two hand-written parts: <out>/method.md (how the test was
run, its limits) and <out>/commentary.txt (what the numbers mean, Markdown). Writes <out>/REPORT.md, <out>/summary.json (for the
website) and <out>/runs.csv (one row per run).
"""
import argparse
import csv
import json
import os
import pathlib
import re
import statistics as st
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent.parent
NYRA = ROOT / "target" / "release" / ("nyra.exe" if os.name == "nt" else "nyra")


def error_block(msg: str) -> str:
    """The first error of a compiler message, whole (message, location, source line, hint), long lines cut."""
    lines = msg.splitlines()
    start = next((i for i, l in enumerate(lines) if re.match(r"\s*error", l)), 0)
    out = []
    for l in lines[start:]:
        if out and (not l.strip() or re.match(r"\s*error", l) or l.startswith("nyra:")):
            break
        out.append(l if len(l) <= 200 else l[:200] + " ...")
    return "\n".join(out)


def build_message(path: pathlib.Path):
    """Builds are deterministic: build the kept file again to get the compiler's whole message (the run record keeps
    only the tail of long messages)."""
    if not NYRA.is_file() or not path.is_file():
        return None
    with tempfile.TemporaryDirectory() as d:
        (pathlib.Path(d) / "site.nyra").write_text(path.read_text(encoding="utf-8"), encoding="utf-8")
        b = subprocess.run([str(NYRA), "build", "site.nyra", "-o", "dist"], cwd=d, capture_output=True, encoding="utf-8", errors="replace")
        return None if b.returncode == 0 else (b.stderr or b.stdout)

MODEL_NAMES = {"claude-opus-5-5": "Opus 5.5", "claude-sonnet-5-5": "Sonnet 5.5", "claude-haiku-4-5-20251001": "Haiku 4.5"}
LANG_NAMES = {"html": "HTML", "nyra": "Nyra"}


def load(out: pathlib.Path):
    rows = []
    for f in sorted((out / "runs").rglob("*.json")):
        r = json.loads(f.read_text(encoding="utf-8"))
        if "total" not in r:
            continue
        base = out / "eval" / r["brief"] / r["model"] / f"{r['lang']}-{r['run']}"
        chk = json.loads(base.with_suffix(".check.json").read_text(encoding="utf-8")) if base.with_suffix(".check.json").exists() else None
        prf = json.loads(base.with_suffix(".perf.json").read_text(encoding="utf-8")) if base.with_suffix(".perf.json").exists() else None
        built = r.get("site") is not None
        tested = built and chk is not None and "error" not in chk
        row = {
            "brief": r["brief"], "model": r["model"], "lang": r["lang"], "run": r["run"],
            "builds_first": r.get("builds_first"), "builds_after_repair": r.get("builds_after_repair"),
            "first_error": r.get("first_error"), "repair_error": r.get("repair_error"),
            "raw_first": out / "raw" / r["brief"] / r["model"] / f"{r['lang']}-{r['run']}.{r['lang']}",
            "raw_fix": out / "raw" / r["brief"] / r["model"] / f"nyra-{r['run']}-fix.nyra",
            "stop_reason": r["first"].get("stop_reason"),
            "built": built, "tested": tested,
            "score": (chk["score"] if tested else 0.0), "passed": chk["passed"] if tested else 0,
            "total_items": chk["total"] if tested else None,
            "failed_items": [i for i in chk["items"] if not i["pass"]] if tested else [],
            "items": {i["id"]: i["pass"] for i in chk["items"]} if tested else {},
            "console_errors": len(chk["console_errors"]) if tested else None,
            "csp_violations": len(chk["csp_violations"]) if tested else None,
            "code_tokens": r.get("code_tokens"), "code_bytes": r.get("code_bytes"), "code_tokens_first": r.get("code_tokens_first"),
            "output_tokens": r["total"]["output_tokens"], "output_tokens_first": r["first"]["output_tokens"],
            "input_tokens": r["total"]["input_tokens"], "cache_read": r["total"]["cache_read"], "cache_write": r["total"]["cache_write"],
            "thinking_tokens_est": r["total"].get("thinking_tokens_est"),
            "seconds": r["total"]["seconds"], "cost_usd": r["total"]["cost_usd"], "calls": r["total"]["calls"],
            "lcp_ms": prf.get("lcp_ms") if prf and "error" not in prf else None,
            "kb": prf.get("kb") if prf and "error" not in prf else None,
            "requests": prf.get("requests") if prf and "error" not in prf else None,
            "final_file": r.get("final_file"),
        }
        rows.append(row)
    return rows


def mean(xs):
    xs = [x for x in xs if x is not None]
    return st.mean(xs) if xs else None


def fmt(x, nd=0, pct=False):
    if x is None:
        return "–"
    if pct:
        return f"{100 * x:.{nd}f}%"
    return f"{x:,.{nd}f}"


def mmm(xs, nd=0, pct=False):
    xs = [x for x in xs if x is not None]
    if not xs:
        return "–"
    if len(xs) == 1:
        return fmt(xs[0], nd, pct)
    return f"{fmt(st.mean(xs), nd, pct)} ({fmt(min(xs), nd, pct)}–{fmt(max(xs), nd, pct)})"


def agg(rs):
    nyra = rs and rs[0]["lang"] == "nyra"
    built_rs = [r for r in rs if r["tested"]]
    return {
        "runs": len(rs),
        "builds_first": sum(1 for r in rs if r["builds_first"]) if nyra else None,
        "builds_after_repair": sum(1 for r in rs if r["builds_after_repair"]) if nyra else None,
        "score_mean": mean([r["score"] for r in rs]), "score_min": min((r["score"] for r in rs), default=None),
        "score_max": max((r["score"] for r in rs), default=None),
        "score_built_mean": mean([r["score"] for r in built_rs]),
        "code_tokens_mean": mean([r["code_tokens"] for r in rs]),
        "output_tokens_mean": mean([r["output_tokens"] for r in rs]),
        "input_tokens_mean": mean([r["input_tokens"] for r in rs]),
        "thinking_tokens_est_mean": mean([r["thinking_tokens_est"] for r in rs]),
        "seconds_mean": mean([r["seconds"] for r in rs]),
        "cost_usd_mean": mean([r["cost_usd"] for r in rs]), "cost_usd_total": sum(r["cost_usd"] for r in rs),
        "lcp_ms_mean": mean([r["lcp_ms"] for r in built_rs]), "kb_mean": mean([r["kb"] for r in built_rs]),
        "console_error_runs": sum(1 for r in built_rs if r["console_errors"]),
        "csp_violation_runs": sum(1 for r in built_rs if r["csp_violations"]),
        "tested": len(built_rs),
    }


def table(groups, title_cols):
    head = title_cols + ["runs", "builds 1st try", "after 1 repair", "checklist (failed build = 0)", "checklist (built only)",
                         "code tokens", "output tokens", "input tokens", "thinking (est.)", "time s", "cost $", "LCP ms", "KB"]
    lines = ["| " + " | ".join(head) + " |", "|" + "---|" * len(head)]
    for key, rs in groups:
        a = agg(rs)
        nyra = rs[0]["lang"] == "nyra"
        lines.append("| " + " | ".join(list(key) + [
            str(a["runs"]),
            f"{a['builds_first']}/{a['runs']}" if nyra else "n/a",
            f"{a['builds_after_repair']}/{a['runs']}" if nyra else "n/a",
            mmm([r["score"] for r in rs], 0, True),
            mmm([r["score"] for r in rs if r["tested"]], 0, True),
            mmm([r["code_tokens"] for r in rs]),
            mmm([r["output_tokens"] for r in rs]),
            fmt(a["input_tokens_mean"]),
            fmt(a["thinking_tokens_est_mean"]),
            mmm([r["seconds"] for r in rs]),
            mmm([r["cost_usd"] for r in rs], 3),
            mmm([r["lcp_ms"] for r in rs if r["tested"]]),
            mmm([r["kb"] for r in rs if r["tested"]]),
        ]) + " |")
    return "\n".join(lines)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    ap.add_argument("--pilot-usd", type=float, default=0.0, help="spent on a pilot outside the results folder (counted in the total)")
    ap.add_argument("--cap", type=float, default=20.0, help="the spend cap of the round (for the cost line)")
    ap.add_argument("--compare", default="", help="an earlier results folder: adds a table of that round next to this one")
    a = ap.parse_args()
    out = pathlib.Path(a.out).resolve()
    rows = load(out)
    gen = json.loads((out / "generation.json").read_text(encoding="utf-8")) if (out / "generation.json").exists() else {}
    models = [m for m in MODEL_NAMES if any(r["model"] == m for r in rows)]
    briefs = sorted({r["brief"] for r in rows})

    def sel(**kw):
        return [r for r in rows if all(r[k] == v for k, v in kw.items())]

    md = ["# Web A/B test: one-file HTML vs one-file Nyra `use web` (held-out briefs)", ""]
    md.append(f"Results folder: `bench/web/results/{out.name}/`. Generated by `bench/web/report.py` from the raw files next to it.")
    md.append("")
    commentary = (out / "commentary.txt").read_text(encoding="utf-8") if (out / "commentary.txt").exists() else ""
    short = [p for p in commentary.split("\n\n") if p.startswith("**In one sentence:**")]
    if short:
        hl = agg(sel(lang="html")), agg(sel(lang="nyra"))
        md += [short[0].replace("**In one sentence:**", "**Short answer:**").replace("\n", " "), "",
               f"HTML: checklist {fmt(hl[0]['score_mean'], 0, True)}, {fmt(hl[0]['code_tokens_mean'])} code tokens, "
               f"${hl[0]['cost_usd_mean']:.3f} per run. Nyra: built {hl[1]['builds_first']}/{hl[1]['runs']} first try and "
               f"{hl[1]['builds_after_repair']}/{hl[1]['runs']} after one repair, checklist {fmt(hl[1]['score_mean'], 0, True)} "
               f"(failed builds = 0; {fmt(hl[1]['score_built_mean'], 0, True)} on built sites), "
               f"{fmt(hl[1]['code_tokens_mean'])} code tokens, ${hl[1]['cost_usd_mean']:.3f} per run.", ""]
    method = out / "method.md"
    if method.exists():
        md += [method.read_text(encoding="utf-8").strip(), ""]
    md += ["## Overall, per model and language (both briefs)", "",
           "Mean (min–max) over runs. Checklist: share of the brief's functional checks passed; a Nyra file that did not "
           "build even after the repair turn scores 0 in the first checklist column and is left out of the second. "
           "Tokens, time and cost of Nyra include the repair turn when there was one. Code tokens: the final file on "
           "the claude-sonnet-5-5 tokenizer. LCP and KB: unthrottled desktop load, median of 3, built sites only.", ""]
    groups = [((MODEL_NAMES[m], LANG_NAMES[l]), sel(model=m, lang=l)) for m in models for l in ("html", "nyra") if sel(model=m, lang=l)]
    md += [table(groups, ["model", "language"]), ""]
    md += ["### All models together", ""]
    md += [table([((LANG_NAMES[l],), sel(lang=l)) for l in ("html", "nyra") if sel(lang=l)], ["language"]), ""]
    for b in briefs:
        md += [f"## Brief: {b}", ""]
        groups = [((MODEL_NAMES[m], LANG_NAMES[l]), sel(brief=b, model=m, lang=l)) for m in models for l in ("html", "nyra") if sel(brief=b, model=m, lang=l)]
        md += [table(groups, ["model", "language"]), ""]
        # per checklist item pass rates
        ids = []
        for r in sel(brief=b):
            for k in r["items"]:
                if k not in ids:
                    ids.append(k)
        names = {}
        for f in (out / "eval" / b).rglob("*.check.json"):
            d = json.loads(f.read_text(encoding="utf-8"))
            for i in d.get("items", []):
                names[i["id"]] = i["name"]
        md += [f"Checklist items, passed / tested sites ({b}):", "",
               "| item | " + " | ".join(f"{MODEL_NAMES[m]} {LANG_NAMES[l]}" for m in models for l in ("html", "nyra")) + " | HTML all | Nyra all |",
               "|---|" + "---|" * (2 * len(models) + 2)]
        for k in ids:
            cells = []
            for m in models:
                for l in ("html", "nyra"):
                    t = [r for r in sel(brief=b, model=m, lang=l) if r["tested"]]
                    cells.append(f"{sum(1 for r in t if r['items'].get(k))}/{len(t)}")
            for l in ("html", "nyra"):
                t = [r for r in sel(brief=b, lang=l) if r["tested"]]
                cells.append(f"{sum(1 for r in t if r['items'].get(k))}/{len(t)}")
            md.append(f"| {names.get(k, k)} | " + " | ".join(cells) + " |")
        md.append("")
    # failures
    md += ["## Failures", "", "### Nyra builds that failed (first error, exactly as the compiler printed it)", ""]
    for r in sel(lang="nyra"):
        if r["builds_first"]:
            continue
        md.append(f"- **{r['brief']} / {MODEL_NAMES[r['model']]} / run {r['run']}** "
                  f"({'built after the repair turn' if r['builds_after_repair'] else 'still failed after the repair turn'}):")
        full = build_message(r["raw_first"])
        n_err = len(re.findall(r"(?m)^\s*error", full or ""))
        md.append("  ```")
        md += ["  " + l for l in (error_block(full) if full else (r["first_error"] or "(no message)")).splitlines()]
        md.append("  ```")
        if n_err > 1:
            md.append(f"  ({n_err} errors in that file in all)")
        if r["repair_error"]:
            full2 = build_message(r["raw_fix"])
            md.append("  After the repair turn:")
            md.append("  ```")
            md += ["  " + l for l in (error_block(full2) if full2 else r["repair_error"]).splitlines()]
            md.append("  ```")
    md += ["", "### Failed checklist items, per run", ""]
    for l in ("html", "nyra"):
        md += [f"#### {LANG_NAMES[l]}", ""]
        for r in sel(lang=l):
            if not r["tested"]:
                md.append(f"- {r['brief']} / {MODEL_NAMES[r['model']]} / run {r['run']}: no site (did not build)")
                continue
            if not r["failed_items"]:
                md.append(f"- {r['brief']} / {MODEL_NAMES[r['model']]} / run {r['run']}: all {r['total_items']} passed")
                continue
            md.append(f"- {r['brief']} / {MODEL_NAMES[r['model']]} / run {r['run']}: {r['passed']}/{r['total_items']}" +
                      (f" (reply stopped: {r['stop_reason']})" if r["stop_reason"] != "end_turn" else ""))
            for i in r["failed_items"]:
                d = i["detail"].replace("|", "/").replace("\n", " ")
                md.append(f"  - {i['name']}: {d[:300]}{'…' if len(d) > 300 else ''}")
        md.append("")
    # cost
    total = sum(r["cost_usd"] for r in rows)
    md += ["## Cost", "", "| model | HTML $ | Nyra $ (incl. repairs) | Nyra repair turns |", "|---|---|---|---|"]
    for m in models:
        h = sum(r["cost_usd"] for r in sel(model=m, lang="html"))
        n = sum(r["cost_usd"] for r in sel(model=m, lang="nyra"))
        rep = sum(1 for r in sel(model=m, lang="nyra") if r["calls"] > 1)
        md.append(f"| {MODEL_NAMES[m]} | {h:.2f} | {n:.2f} | {rep} |")
    pilot = f"; with the pilot described under Method (${a.pilot_usd:.2f}) the round spent ${total + a.pilot_usd:.2f}" if a.pilot_usd else ""
    md += ["", f"Total of this round's runs: **${total:.2f}** (cap ${a.cap:.0f}{pilot}). Runs done: {len(rows)} of {gen.get('started_jobs', '?')} planned; "
           f"skipped at the cap: {gen.get('skipped_cap', 0)}; API errors: {len(gen.get('errors', []))}.", ""]
    if a.compare:
        prev_dir = pathlib.Path(a.compare).resolve()
        prev = load(prev_dir)
        pb = sorted({r["brief"] for r in prev})

        def cell(rs):
            x = agg(rs)
            nyra = rs[0]["lang"] == "nyra"
            return [f"{x['builds_first']}/{x['runs']}" if nyra else "n/a", f"{x['builds_after_repair']}/{x['runs']}" if nyra else "n/a",
                    fmt(x["score_mean"], 0, True), fmt(x["score_built_mean"], 0, True), fmt(x["code_tokens_mean"]),
                    fmt(x["output_tokens_mean"]), f"{x['cost_usd_mean']:.3f}"]
        md += [f"## Round {prev_dir.name} vs round {out.name} (same method, different held-out briefs)", "",
               f"Earlier round: briefs {', '.join(pb)} (`bench/web/results/{prev_dir.name}/`). This round: briefs {', '.join(briefs)}. "
               "Same models, settings, prompt template, repair rule and checker rules; Nyra was fixed in between (see the "
               "commentary). The briefs differ, so this is not a pure A/B of the fixes: a change can come from the fixes, the "
               "briefs or chance (4 runs per cell).", "",
               "| model | language | round | builds 1st try | after 1 repair | checklist (failed build = 0) | checklist (built only) | code tokens | output tokens | cost $ per run |",
               "|---|---|---|---|---|---|---|---|---|---|"]
        for m in models:
            for l in ("html", "nyra"):
                for name, rs_all in ((prev_dir.name, prev), (out.name, rows)):
                    rs = [r for r in rs_all if r["model"] == m and r["lang"] == l]
                    if rs:
                        md.append(f"| {MODEL_NAMES[m]} | {LANG_NAMES[l]} | {name} | " + " | ".join(cell(rs)) + " |")
        for l in ("html", "nyra"):
            for name, rs_all in ((prev_dir.name, prev), (out.name, rows)):
                rs = [r for r in rs_all if r["lang"] == l]
                if rs:
                    md.append(f"| all | {LANG_NAMES[l]} | {name} | " + " | ".join(cell(rs)) + " |")
        md.append("")
    findings = out / "commentary.txt"
    if findings.exists():
        md += [findings.read_text(encoding="utf-8").strip(), ""]
    (out / "REPORT.md").write_text("\n".join(md), encoding="utf-8", newline="\n")

    # compact summary for the website
    def block(rs):
        x = agg(rs)
        return {k: (round(v, 4) if isinstance(v, float) else v) for k, v in x.items()}
    summary = {
        "schema": "nyra-web-ab/1", "date": out.name, "briefs": briefs, "held_out": True,
        "viewport": "1280x900 desktop", "runs_per_cell": max((r["run"] for r in rows), default=0),
        "models": {MODEL_NAMES[m]: m for m in models},
        "effort": gen.get("effort"), "repair_turns": "Nyra only, at most one, with the compiler message; HTML has no compiler",
        "by_model": {MODEL_NAMES[m]: {LANG_NAMES[l]: block(sel(model=m, lang=l)) for l in ("html", "nyra") if sel(model=m, lang=l)} for m in models},
        "by_language": {LANG_NAMES[l]: block(sel(lang=l)) for l in ("html", "nyra") if sel(lang=l)},
        "by_brief": {b: {LANG_NAMES[l]: block(sel(brief=b, lang=l)) for l in ("html", "nyra") if sel(brief=b, lang=l)} for b in briefs},
        "total_cost_usd": round(total, 4),
    }
    (out / "summary.json").write_text(json.dumps(summary, indent=1), encoding="utf-8", newline="\n")
    with open(out / "runs.csv", "w", newline="", encoding="utf-8") as fh:
        cols = ["brief", "model", "lang", "run", "builds_first", "builds_after_repair", "built", "score", "passed", "total_items",
                "code_tokens", "code_bytes", "output_tokens", "output_tokens_first", "input_tokens", "cache_read", "cache_write",
                "thinking_tokens_est", "seconds", "cost_usd", "calls", "lcp_ms", "kb", "requests", "console_errors", "csp_violations",
                "stop_reason", "final_file"]
        w = csv.DictWriter(fh, fieldnames=cols, extrasaction="ignore")
        w.writeheader()
        for r in rows:
            w.writerow(r)
    print(f"REPORT.md, summary.json, runs.csv: {len(rows)} runs, ${total:.2f}")


if __name__ == "__main__":
    main()
