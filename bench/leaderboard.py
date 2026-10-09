#!/usr/bin/env python3
"""A static leaderboard page from the published summaries, for the website (nyralang.dev) to host.

    python bench/leaderboard.py                       # bench/published/*.json -> results.html + results.json
    python bench/leaderboard.py --out site/benchmark  # write them somewhere else
    python bench/publish.py ... --name 2026-10-v0.6 --leaderboard   # publish a run, then rebuild the page

`bench/published/<name>.json` files (written by publish.py) are the only input. The page is one self-contained HTML file:
no external requests, no framework, light and dark colours, sortable columns when JavaScript is on and a readable table
when it is not. `results.json` next to it holds the numbers the page shows, for anything else that wants them.

What it shows, per published run: for every model and language the first-try success with its 95% interval (the
task-level bootstrap for the v2 and edit tiers, the Wilson interval otherwise), the success within the repairs, the code
tokens, the cost; for the v2 tier the programs that passed the example and failed a hidden input; for the edit tier the
output tokens per successful edit. Mock runs are skipped (they measure nothing) unless --include-mock.

Standard library only.
"""

from __future__ import annotations

import argparse
import datetime as dt
import html
import json
import sys
from pathlib import Path
from typing import Optional

BENCH_DIR = Path(__file__).resolve().parent
if str(BENCH_DIR) not in sys.path:
    sys.path.insert(0, str(BENCH_DIR))

import report  # noqa: E402

PUBLISHED_DIR = BENCH_DIR / "published"
OUTPUT_NAMES = ("results.html", "results.json")
TIER_TITLES = {"v1": "Original tasks (no input)", "v2": "Stdin tasks with hidden inputs", "edit": "Editing an existing program",
               "safety": "Safety"}


def load_published(directory: Path, include_mock: bool = False) -> list:
    """The published summaries (newest name last), as dicts; files that are not summaries are skipped."""
    out = []
    for path in sorted(Path(directory).glob("*.json")):
        if path.name in OUTPUT_NAMES:
            continue
        try:
            data = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            continue
        if not (isinstance(data, dict) and isinstance(data.get("models"), dict) and isinstance(data.get("run"), dict)):
            continue
        if data.get("mock") and not include_mock:
            continue
        data["_file"] = path.name
        out.append(data)
    return out


def _interval(stats: dict, block: Optional[dict], key: str) -> tuple:
    """(low, high, how) of a rate: the task-level bootstrap when the run has one (`block` is the tier's block for it), else
    the Wilson interval of the headline statistics."""
    if block and block.get("boot_ci"):
        return block["boot_ci"][0], block["boot_ci"][1], "task bootstrap"
    ci = stats.get(f"{key}_ci")
    if ci:
        return ci[0], ci[1], "Wilson"
    return None, None, ""


def rows_of(entry: dict) -> list:
    """One dict per (model, language) of a published run, with everything the page shows."""
    tier = entry["run"].get("tier") or "v1"
    rows = []
    for model, m in entry["models"].items():
        tier_block = m.get("edit") if tier == "edit" else m.get("v2")
        for lang in report.ordered_langs(m["langs"]):
            s = m["langs"][lang]
            tb = (tier_block or {}).get(lang) or {}
            n = s.get("n") or 0
            lo, hi, how = _interval(s, tb.get("pass_at_1"), "pass_at_1")
            lo2, hi2, _ = _interval(s, tb.get("pass_within_repairs"), "pass_within_repairs")
            rows.append({
                "model": model, "lang": lang, "runs": n, "tasks": s.get("tasks"),
                "pass_at_1": s.get("pass_at_1"), "pass_at_1_rate": s.get("pass_at_1_rate"), "pass_at_1_low": lo, "pass_at_1_high": hi,
                "interval": how,
                "within": s.get("pass_within_repairs"), "within_rate": s.get("pass_within_repairs_rate"),
                "within_low": lo2, "within_high": hi2,
                "code_tokens": s.get("avg_code_tokens_first_attempt"), "billed_tokens": s.get("avg_output_tokens_first_attempt"),
                "cost_per_run_usd": s.get("avg_cost_per_run_usd"),
                "example_only": tb.get("example_only"), "example_passes": tb.get("example_passes"),
                "tokens_per_success": tb.get("output_tokens_per_success"),
                "tokens_vs_baseline": tb.get("tokens_per_success_vs_baseline"),
                "edits_passed": tb.get("passed"),
            })
    return rows


# ------------------------------------------------------------------------------------------------ rendering

CSS = """
:root{--bg:#fbfaf8;--fg:#1d1c1a;--muted:#6a665f;--line:#e4e0d8;--card:#fff;--accent:#2f6f5e;--bar:#9fcfc0;--whisker:#2f6f5e;--warn:#9a5b00}
@media (prefers-color-scheme:dark){:root:not([data-theme=light]){--bg:#161513;--fg:#ece9e2;--muted:#9a958b;--line:#2d2b27;--card:#1e1d1a;--accent:#79c3ad;--bar:#2d5a4d;--whisker:#79c3ad;--warn:#e0a458}}
:root[data-theme=dark]{--bg:#161513;--fg:#ece9e2;--muted:#9a958b;--line:#2d2b27;--card:#1e1d1a;--accent:#79c3ad;--bar:#2d5a4d;--whisker:#79c3ad;--warn:#e0a458}
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--fg);font:16px/1.5 system-ui,-apple-system,"Segoe UI",Roboto,sans-serif}
main{max-width:1040px;margin:0 auto;padding:32px 16px 64px}
h1{font-size:1.9rem;margin:0 0 4px}h2{font-size:1.25rem;margin:40px 0 4px}h3{font-size:1rem;margin:24px 0 8px}
p,li{max-width:70ch}.muted{color:var(--muted)}.small{font-size:.875rem}
.card{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:16px;margin:12px 0}
.wrap{overflow-x:auto}
table{border-collapse:collapse;width:100%;font-size:.9rem}
th,td{padding:7px 10px;border-bottom:1px solid var(--line);text-align:left;vertical-align:middle;white-space:nowrap}
th{font-weight:600;color:var(--muted);cursor:pointer;user-select:none}th[data-nosort]{cursor:default}
td.num,th.num{text-align:right;font-variant-numeric:tabular-nums}
.bar{position:relative;height:10px;width:140px;background:var(--line);border-radius:5px;display:inline-block;vertical-align:middle;margin-right:8px}
.bar>i{position:absolute;left:0;top:0;bottom:0;background:var(--bar);border-radius:5px}
.bar>b{position:absolute;top:-2px;bottom:-2px;border-left:2px solid var(--whisker);border-right:2px solid var(--whisker);opacity:.8}
.bar>b::after{content:"";position:absolute;left:0;right:0;top:50%;border-top:2px solid var(--whisker)}
.tag{display:inline-block;padding:0 7px;border:1px solid var(--line);border-radius:99px;font-size:.78rem;color:var(--muted);margin-right:4px}
.warn{color:var(--warn)}
footer{margin-top:48px;border-top:1px solid var(--line);padding-top:16px}
"""

SORT_JS = """
document.querySelectorAll('table.sortable').forEach(function(t){
  t.querySelectorAll('th:not([data-nosort])').forEach(function(th,i){
    th.addEventListener('click',function(){
      var rows=Array.prototype.slice.call(t.tBodies[0].rows), dir=th.dataset.dir==='asc'?-1:1;
      t.querySelectorAll('th').forEach(function(x){delete x.dataset.dir});th.dataset.dir=dir===1?'asc':'desc';
      rows.sort(function(a,b){var x=a.cells[i].dataset.v,y=b.cells[i].dataset.v,nx=parseFloat(x),ny=parseFloat(y);
        if(!isNaN(nx)&&!isNaN(ny))return (nx-ny)*dir;return String(x).localeCompare(String(y))*dir});
      rows.forEach(function(r){t.tBodies[0].appendChild(r)});
    });
  });
});
"""


def esc(x) -> str:
    return html.escape(str(x), quote=True)


def pct(rate: Optional[float]) -> str:
    return "n/a" if rate is None else f"{100 * rate:.0f}%"


def num(x: Optional[float], digits: int = 0) -> str:
    return "n/a" if x is None else f"{x:,.{digits}f}"


def usd(x: Optional[float]) -> str:
    return "n/a" if x is None else (f"${x:,.4f}" if abs(x) < 1 else f"${x:,.2f}")


def rate_cell(rate, lo, hi, k=None, n=None) -> str:
    """A cell with a bar for the rate and a whisker for its interval; the sort key is the rate."""
    if rate is None:
        return '<td data-v="-1">n/a</td>'
    whisker = ""
    text = f"{pct(rate)}"
    if lo is not None and hi is not None:
        whisker = f'<b style="left:{100 * lo:.1f}%;width:{max(0.5, 100 * (hi - lo)):.1f}%"></b>'
        text += f' <span class="muted small">[{100 * lo:.0f}-{100 * hi:.0f}%]</span>'
    if k is not None and n:
        text += f' <span class="muted small">{k}/{n}</span>'
    return (f'<td data-v="{rate:.4f}"><span class="bar"><i style="width:{100 * rate:.1f}%"></i>{whisker}</span>{text}</td>')


def table(header: list, rows: list, sortable: bool = True) -> str:
    head = "".join(f'<th class="{c}">{esc(h)}</th>' for h, c in header)
    return (f'<div class="wrap"><table class="{"sortable" if sortable else ""}"><thead><tr>{head}</tr></thead><tbody>'
            + "".join(f"<tr>{r}</tr>" for r in rows) + "</tbody></table></div>")


def td(text, v=None, cls="") -> str:
    return f'<td class="{cls}" data-v="{esc(v if v is not None else text)}">{esc(text)}</td>'


def run_section(entry: dict) -> str:
    run = entry["run"]
    tier = run.get("tier") or "v1"
    rows = rows_of(entry)
    langs = report.ordered_langs({r["lang"] for r in rows})
    names = [report.display(lang) for lang in langs]
    facts = [TIER_TITLES.get(tier, tier)]
    tasks = run.get("tasks") or []
    if tasks:
        facts.append(f"{', '.join(str(t) for t in tasks)} tasks")
    facts.append(f"{run.get('samples', '?')} sample(s) per task")
    facts.append(f"up to {run.get('repairs', '?')} repair(s)")
    if (run.get("nyra") or {}).get("version"):
        facts.append(run["nyra"]["version"])
    dates = ", ".join(run.get("dates") or [])
    if dates:
        facts.append(dates)
    out = [f'<section id="{esc(entry.get("name", entry["_file"]))}"><h2>{esc(entry.get("name", entry["_file"]))}</h2>',
           '<p class="muted small">' + " &middot; ".join(esc(f) for f in facts) + "</p>"]
    for note in entry.get("notes") or []:
        out.append(f'<p class="warn small">{esc(note)}</p>')
    if not entry.get("complete", True):
        out.append('<p class="warn small">Incomplete: at least one run was stopped early.</p>')

    header = [("Model", ""), ("Language", ""), ("First try", ""), ("Within repairs", ""), ("Code tokens", "num"),
              ("Billed tokens", "num"), ("Cost / run", "num")]
    if tier == "v2":
        header.append(("Example right, hidden wrong", "num"))
    if tier == "edit":
        header = [("Model", ""), ("Arm", ""), ("First try", ""), ("Within repairs", ""), ("Output tokens / successful edit", "num"),
                  ("vs first arm", "num"), ("Cost / run", "num")]
    body = []
    for r in sorted(rows, key=lambda r: (r["model"], langs.index(r["lang"]))):
        cells = [td(r["model"]), td(report.display(r["lang"])),
                 rate_cell(r["pass_at_1_rate"], r["pass_at_1_low"], r["pass_at_1_high"], r["pass_at_1"], r["runs"]),
                 rate_cell(r["within_rate"], r["within_low"], r["within_high"], r["within"], r["runs"])]
        if tier == "edit":
            cells += [td(num(r["tokens_per_success"]), r["tokens_per_success"] if r["tokens_per_success"] is not None else -1, "num"),
                      td("n/a" if r["tokens_vs_baseline"] is None else f"{r['tokens_vs_baseline']:.2f}x",
                         r["tokens_vs_baseline"] if r["tokens_vs_baseline"] is not None else -1, "num"),
                      td(usd(r["cost_per_run_usd"]), r["cost_per_run_usd"] if r["cost_per_run_usd"] is not None else -1, "num")]
        else:
            cells += [td(num(r["code_tokens"]), r["code_tokens"] if r["code_tokens"] is not None else -1, "num"),
                      td(num(r["billed_tokens"]), r["billed_tokens"] if r["billed_tokens"] is not None else -1, "num"),
                      td(usd(r["cost_per_run_usd"]), r["cost_per_run_usd"] if r["cost_per_run_usd"] is not None else -1, "num")]
            if tier == "v2":
                eo = r["example_only"]
                cells.append(td("n/a" if eo is None else f"{eo} of {r['example_passes']}", eo if eo is not None else -1, "num"))
        body.append("".join(cells))
    out.append(table(header, body))
    out.append('<p class="muted small">The bar is the first-try rate; the whisker is its 95% interval ('
               + ("a bootstrap over tasks, the unit of evidence: repeating a task does not add a task" if tier in ("v2", "edit")
                  else "a Wilson interval with the number of tasks as its sample size") + ").</p>")
    cmp_rows = nyra_vs_python(entry)
    if cmp_rows:
        out.append("<h3>Nyra against Python</h3>" + table(
            [("Model", ""), ("Nyra first try", "num"), ("Python first try", "num"), ("Nyra code tokens / Python's", "num")], cmp_rows))
    out.append("</section>")
    return "".join(out)


def nyra_vs_python(entry: dict) -> list:
    rows = []
    for model, m in entry["models"].items():
        a, b = m["langs"].get("nyra"), m["langs"].get("python")
        if not a or not b:
            continue
        ratio = (a.get("avg_code_tokens_first_attempt") / b["avg_code_tokens_first_attempt"]
                 if a.get("avg_code_tokens_first_attempt") and b.get("avg_code_tokens_first_attempt") else None)
        rows.append(td(model) + td(pct(a.get("pass_at_1_rate")), a.get("pass_at_1_rate") or -1, "num")
                    + td(pct(b.get("pass_at_1_rate")), b.get("pass_at_1_rate") or -1, "num")
                    + td("n/a" if ratio is None else f"{ratio:.2f}x", ratio if ratio is not None else -1, "num"))
    return rows


def build_page(entries: list, generated: Optional[str] = None) -> str:
    generated = generated or dt.date.today().isoformat()
    sections = "".join(run_section(e) for e in entries) or '<p class="muted">No published results yet.</p>'
    if any(e.get("mock") for e in entries):
        intro = ('<strong class="warn">This page includes MOCK runs: a self-test of the pipeline that replays the reference '
                 'solutions, not a measurement.</strong>')
    else:
        intro = "Every number comes from a real run; mock runs are never shown."
    toc = " ".join(f'<a class="tag" href="#{esc(e.get("name", e["_file"]))}">{esc(e.get("name", e["_file"]))}</a>' for e in entries)
    return f"""<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Nyra benchmark results</title>
<meta name="description" content="How often language models write a correct Nyra program on the first try, compared with Python, TypeScript and Rust.">
<style>{CSS}</style></head>
<body><main>
<h1>Nyra benchmark results</h1>
<p class="muted">Generated {esc(generated)} from the summaries in <code>bench/published/</code>. {intro}</p>
<div class="card small">
<strong>How to read this.</strong> A program passes a task only if its output is exactly right; in the stdin tier that means on
every input, including hidden ones the model never saw, so printing the example's answer fails. <em>First try</em> is the share of runs whose first program passed; <em>within repairs</em> allows the model to retry after seeing
the compiler's or interpreter's messages. <em>Code tokens</em> count the program alone with the model's own tokenizer; <em>billed
tokens</em> are what the API charged for the reply, thinking included. Nyra is given its language spec in the prompt; the other
languages rely on what the model already knows. Intervals are 95%; with few tasks they are wide. Method, tasks and caveats:
<code>bench/README.md</code>.
</div>
<p>{toc}</p>
{sections}
<footer class="muted small">Static page produced by <code>bench/leaderboard.py</code>. Prompts, replies and programs are not published.</footer>
</main><script>{SORT_JS}</script></body></html>
"""


def build_json(entries: list, generated: Optional[str] = None) -> dict:
    return {"generated": generated or dt.date.today().isoformat(),
            "runs": [{"name": e.get("name", e["_file"]), "file": e["_file"], "tier": e["run"].get("tier") or "v1",
                      "complete": e.get("complete", True), "rows": rows_of(e)} for e in entries]}


def generate(published_dir: Path = PUBLISHED_DIR, out_dir: Optional[Path] = None, include_mock: bool = False,
             generated: Optional[str] = None) -> list:
    """Write results.html and results.json; returns the paths."""
    out_dir = Path(out_dir) if out_dir else Path(published_dir)
    entries = load_published(published_dir, include_mock)
    out_dir.mkdir(parents=True, exist_ok=True)
    html_path, json_path = out_dir / "results.html", out_dir / "results.json"
    html_path.write_text(build_page(entries, generated), encoding="utf-8", newline="\n")
    json_path.write_text(json.dumps(build_json(entries, generated), indent=2, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n")
    return [html_path, json_path]


def main(argv=None) -> int:
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8", errors="replace")
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0], epilog=__doc__.split("\n\n", 1)[1],
                                formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--published", default=str(PUBLISHED_DIR), help="folder of summaries (default: bench/published)")
    p.add_argument("--out", help="folder for results.html and results.json (default: the same folder)")
    p.add_argument("--include-mock", action="store_true", help="also show mock runs (a pipeline demonstration, not a measurement)")
    args = p.parse_args(argv)
    paths = generate(Path(args.published), Path(args.out) if args.out else None, args.include_mock)
    print("wrote " + "\n      ".join(str(x) for x in paths))
    return 0


if __name__ == "__main__":
    sys.exit(main())
