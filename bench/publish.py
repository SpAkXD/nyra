#!/usr/bin/env python3
"""Turn raw benchmark result files into the summary that is committed to the repository.

    python bench/publish.py bench/results/2026-10-07-openrouter-*.json --name 2026-10-v0.4
    python bench/publish.py bench/results/2026-10-07-openrouter-compare.json --name 2026-10-v0.4 --stdout

A raw result file (bench/results/, git-ignored) holds every prompt, every reply and every program the models
wrote. What this script writes to bench/published/<name>.md and <name>.json is the part worth publishing:

  * the headline per model x language: first-try success, success within the repairs, code tokens with the
    billed output tokens next to them, cost;
  * Nyra against each other language on the runs both got right first try;
  * a breakdown per task category;
  * a few notable failures: the tasks that never passed, compiler bugs, the Nyra compiler errors models hit
    most, the tasks hardest for Nyra on the first try.

No prompt, reply or program is copied. The sources are named with the checksum of the raw file, so anyone who
holds the raw files can check that the summary belongs to them.

Mock runs are refused (they replay the reference solutions and measure nothing), and so are incomplete runs
and files that were not measured the same way (other tasks, other spec, other compiler, other number of
samples or repairs): such results are not comparable. --allow-mock, --allow-incomplete and --allow-mismatch
override that, and the summary then says so.
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import re
import sys
from collections import Counter
from pathlib import Path
from typing import Optional

BENCH_DIR = Path(__file__).resolve().parent
if str(BENCH_DIR) not in sys.path:
    sys.path.insert(0, str(BENCH_DIR))

import report  # noqa: E402

PUBLISHED_DIR = BENCH_DIR / "published"
SCHEMA_VERSION = 1
# What must be equal for two result files to be comparable.
SAME_EXPERIMENT = (("tasks_sha256", "task set"), ("langs", "languages"), ("repairs", "repairs"),
                   ("samples", "samples per task"), ("max_version", "task version limit"),
                   ("timeout_s", "time limit"))


class PublishError(Exception):
    """The results cannot be published (or the command line is wrong)."""


# ------------------------------------------------------------------------------ loading


def _read_json(path: Path):
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as exc:
        raise PublishError(f"{path}: cannot read as JSON: {exc}") from None


def _is_results(data) -> bool:
    return isinstance(data, dict) and isinstance(data.get("run"), dict) and isinstance(data.get("records"), list) \
        and isinstance(data.get("summary"), dict)


def load_results(paths: list) -> list:
    """[(path, results dict, sha256 of the file)]. A compare index (written next to a multi-model run) stands for
    the result files it lists."""
    loaded: list = []
    seen: set = set()

    def add(path: Path):
        path = path.resolve()
        if path in seen:
            return
        seen.add(path)
        data = _read_json(path)
        if _is_results(data):
            loaded.append((path, data, hashlib.sha256(path.read_bytes()).hexdigest()))
        elif isinstance(data, dict) and isinstance(data.get("models"), list):
            for entry in data["models"]:
                name = entry.get("file") if isinstance(entry, dict) else None
                if not isinstance(name, str) or Path(name).name != name:
                    raise PublishError(f"{path.name}: bad entry in the model list: {entry!r}")
                add(path.parent / name)
        else:
            raise PublishError(f"{path}: not a benchmark result file")

    for p in paths:
        add(Path(p))
    if not loaded:
        raise PublishError("no result files given")
    return loaded


def check_comparable(results_list: list) -> list:
    """Why these results should not be put side by side (empty list: they can)."""
    problems = []
    for key, label in SAME_EXPERIMENT:
        values = {json.dumps(r["run"].get(key)) for r in results_list}
        if len(values) > 1:
            problems.append(f"{label} differs between the result files")
    for label, getter in (("Nyra compiler", lambda run: (run.get("nyra") or {}).get("version")),
                          ("Nyra spec", lambda run: (run.get("spec") or {}).get("sha256"))):
        if len({getter(r["run"]) for r in results_list}) > 1:
            problems.append(f"{label} differs between the result files")
    models = [report.model_label(r) for r in results_list]
    dupes = sorted({m for m in models if models.count(m) > 1})
    if dupes:
        problems.append(f"the same model appears twice: {', '.join(dupes)}")
    return problems


# ---------------------------------------------------------------------- notable failures


def _clip(text: str, n: int = 160) -> str:
    text = re.sub(r"\s+", " ", text or "").strip()
    return text if len(text) <= n else text[:n - 3] + "..."


def failure_message(lang: str, result: dict) -> str:
    """One line saying why an attempt failed: the compiler's message for Nyra, the interesting line of stderr
    for the others. Never program text."""
    errors = result.get("errors") or []
    if errors:
        e = errors[0]
        return _clip(f"{e.get('code', '?')}: {e.get('message', '')}")
    lines = [ln.strip() for ln in (result.get("stderr") or "").splitlines() if ln.strip()]
    if lines:
        if lang == "rust":
            first = next((ln for ln in lines if ln.startswith("error")), lines[0])
            return _clip(first)
        return _clip(lines[-1])
    return {"wrong_output": "the program ran but printed the wrong output",
            "no_code": "no fenced code block in the reply",
            "timeout": "the program did not finish in time"}.get(result.get("kind", ""), result.get("kind", "?"))


def notable_failures(results: dict, limit: int = 12) -> list:
    """The runs of one model worth a look: compiler bugs first, then runs that never passed (Nyra first)."""
    langs = report.ordered_langs(results["run"]["langs"])
    out = []
    for rec in results["records"]:
        if rec["status"] not in ("pass", "fail") or not rec["attempts"]:
            continue
        kinds = [a["result"]["kind"] for a in rec["attempts"]]
        bug = "toolchain_error" in kinds
        if rec["status"] != "fail" and not bug:
            continue
        last = rec["attempts"][-1]["result"]
        worst = next((a["result"] for a in rec["attempts"] if a["result"]["kind"] == "toolchain_error"), last)
        codes = sorted({e.get("code", "?") for a in rec["attempts"] for e in a["result"].get("errors", [])})
        out.append({
            "lang": rec["lang"], "task": rec["task_id"], "sample": rec["sample"],
            "outcome": ("compiler or toolchain bug" if bug else "never passed"),
            "attempts": rec["attempts_used"], "failure_kinds": kinds, "error_codes": codes,
            "message": failure_message(rec["lang"], worst),
        })
    out.sort(key=lambda e: (e["outcome"] != "compiler or toolchain bug", langs.index(e["lang"]) if e["lang"] in langs else 9,
                            e["task"], e["sample"]))
    return out[:limit]


def common_nyra_errors(results_list: list, limit: int = 8) -> list:
    """The Nyra compiler errors on first attempts across all models: [{code, count, message, task}]."""
    counts: Counter = Counter()
    example: dict = {}
    for results in results_list:
        for rec in results["records"]:
            if rec["lang"] != "nyra" or rec["status"] not in ("pass", "fail") or not rec["attempts"]:
                continue
            for e in rec["attempts"][0]["result"].get("errors", []):
                code = e.get("code", "?")
                counts[code] += 1
                example.setdefault(code, {"message": _clip(e.get("message", "")), "task": rec["task_id"]})
    return [{"code": c, "count": n, **example[c]} for c, n in sorted(counts.items(), key=lambda kv: (-kv[1], kv[0]))[:limit]]


def hardest_tasks(results_list: list, limit: int = 8) -> list:
    """Tasks with the lowest first-try rate for the baseline language, over every model and sample:
    [{task, category, rates: {lang: [passed, runs]}}]."""
    langs = report.comparison_langs(results_list)
    rates: dict = {}
    category: dict = {}
    for results in results_list:
        run_langs = results["run"]["langs"]
        by_key: dict = {}
        for rec in results["records"]:
            by_key.setdefault((rec["task_id"], rec["sample"]), {})[rec["lang"]] = rec
        for (task, _), recs in by_key.items():
            if not all(l in recs and recs[l]["status"] in ("pass", "fail") for l in run_langs):
                continue
            category[task] = (results["run"].get("tasks") or {}).get(task, {}).get("category", "")
            for lang in run_langs:
                cell = rates.setdefault(task, {}).setdefault(lang, [0, 0])
                cell[0] += 1 if recs[lang]["first_try"] else 0
                cell[1] += 1
    base = langs[0]

    def key(task):
        cell = rates[task].get(base) or [0, 1]
        return (cell[0] / cell[1], task)

    ranked = [t for t in sorted(rates, key=key) if any(c[0] < c[1] for c in rates[t].values())]
    return [{"task": t, "category": category.get(t, ""), "rates": rates[t]} for t in ranked[:limit]]


# --------------------------------------------------------------------------- rendering


def _cell(text) -> str:
    """Make arbitrary text safe inside a Markdown table cell."""
    return re.sub(r"\s+", " ", str(text)).replace("|", "\\|")


def _ratio_cell(pair: Optional[dict], base: str, other: str) -> str:
    if not pair or not pair["n"]:
        return "-"
    a, b = pair["langs"][base]["avg_code_tokens"], pair["langs"][other]["avg_code_tokens"]
    ratio = report._ratio(a, b)
    return f"{ratio} (n={pair['n']})"


def _nyra_vs_table(results_list: list, langs: list, h: str) -> list:
    base = langs[0]
    others = langs[1:]
    if not others:
        return []
    rows = []
    for results in results_list:
        paired = results["summary"].get("paired") or {}
        pairs = paired.get("pairs") or {}
        rows.append([report.model_label(results)] + [_ratio_cell(pairs.get(o), base, o) for o in others])
    return ([f"{h} {report.display(base)} code tokens relative to the other languages", "",
             f"Code tokens of {report.display(base)} divided by code tokens of the other language, averaged over the "
             f"runs both got right on the first try (n = number of such runs). Below 1.00x means {report.display(base)} "
             "needs fewer tokens for the same program.", ""]
            + report._table(["Model"] + [report.display(o) for o in others], rows) + [""])


def _failure_table(entries: list) -> list:
    if not entries:
        return ["None.", ""]
    rows = [[report.display(e["lang"]), e["task"], e["outcome"], e["attempts"], ", ".join(e["error_codes"]) or "-",
             _cell(e["message"])] for e in entries]
    return report._table(["Language", "Task", "Outcome", "Attempts", "Error codes", "Why"], rows) + [""]


def render_publication(name: str, loaded: list, notes: list, failures_per_model: int = 12) -> tuple:
    """(Markdown, data) for a list of loaded results. `notes` are warnings about the set (printed in the file)."""
    results_list = [r for _, r, _ in loaded]
    langs = report.comparison_langs(results_list)
    first = results_list[0]["run"]
    mock = any(r["run"].get("mock") for r in results_list)
    complete = all(r["run"].get("complete", True) for r in results_list)
    dates = sorted({r["run"].get("date") for r in results_list if r["run"].get("date")})
    started = min((r["run"].get("started_at") or "" for r in results_list), default="")
    finished = max((r["run"].get("finished_at") or "" for r in results_list), default="")
    nyra = first.get("nyra") or {}
    spec = first.get("spec") or {}
    commits = sorted({(r["run"].get("repo") or {}).get("commit") or "unknown" for r in results_list})
    dirty = any((r["run"].get("repo") or {}).get("dirty") for r in results_list)
    tasks_n = sorted({r["summary"]["tasks"] for r in results_list})
    repairs = first["repairs"]

    md = [f"# Nyra benchmark results: {name}", "",
          f"> A summary generated by `bench/publish.py` from {len(loaded)} raw result file(s). Prompts, replies and the "
          "programs the models wrote stay on the machine that ran the benchmark; only the numbers below are published. "
          "Method and caveats: `bench/README.md`.", ""]
    if mock:
        md += ["> **MOCK RESULTS: a self-test of the pipeline, not a measurement.** The \"models\" replayed the reference "
               "solutions. Do not quote any number below.", ""]
    if not complete:
        md += ["> **INCOMPLETE:** at least one run was stopped early; its numbers cover only the runs that finished.", ""]
    for n in notes:
        md += [f"> Warning: {n}", ""]

    md += ["## What was measured", ""]
    facts = [f"Languages: {', '.join(report.display(l) for l in langs)}. "
             f"Tasks: {', '.join(str(t) for t in tasks_n)} deterministic programs with exact expected output "
             f"(task set `{(first.get('tasks_sha256') or '?')[:12]}`).",
             f"Per model and language: {first['samples']} sample(s) per task, up to {repairs} repair(s) after a failed "
             f"first try, {first['timeout_s']:g} s per program. Every cell counts only runs where all languages of that "
             "model produced a valid result.",
             f"Models ({len(results_list)}): " + ", ".join(
                 f"`{report.model_label(r)}`" + (f" (served as `{', '.join(r['run']['served_models'])}`)"
                                                  if r["run"].get("served_models") and not r["run"].get("mock")
                                                  and r["run"]["served_models"] != [report.model_label(r)] else "")
                 for r in results_list) + ".",
             f"Run on {', '.join(dates) or '?'} ({started[:19]} to {finished[:19]} UTC), harness commit "
             f"{', '.join(commits)}{' with uncommitted changes' if dirty else ''}."]
    tools = []
    if nyra.get("version"):
        tools.append(f"{nyra['version']}" + (f", {first['backend']} backend" if first.get("backend") else "")
                     + (f", spec `{spec['sha256'][:12]}`" if spec.get("sha256") else ""))
    if first.get("node"):
        tools.append("Node.js " + first["node"]["version"].lstrip("v"))
    if first.get("rust"):
        tools.append(first["rust"]["version"])
    if first.get("python"):
        tools.append("Python " + first["python"])
    if tools:
        facts.append("Tools: " + "; ".join(tools) + ".")
    md += [f"- {f}" for f in facts] + [""]

    md += report.comparison_tables(results_list, "##")
    md += _nyra_vs_table(results_list, langs, "##")

    md += ["## Per model", ""]
    for results in results_list:
        run, summary = results["run"], results["summary"]
        md += [f"### {report.model_label(results)}", ""]
        if not summary["runs"]:
            md += ["No complete results.", ""]
            continue
        stats = summary["langs"]
        mlangs = run["langs"]
        names = [report.display(l) for l in mlangs]
        md += report._table(["Metric"] + names, report.headline_rows(stats, mlangs, repairs, full=False)) + [""]
        if summary.get("paired") and len(mlangs) > 1:
            md += report._render_paired(summary["paired"], mlangs, names, summary["runs"], "####")
        if summary.get("by_category"):
            md += report._render_categories(summary["by_category"], mlangs, names, repairs, "####")

    md += ["## Notable failures", ""]
    hard = hardest_tasks(results_list)
    if hard:
        md += [f"### Tasks hardest for {report.display(langs[0])} on the first try (all models and samples)", ""]
        rows = [[h["task"], h["category"]] + [(f"{h['rates'][l][0]}/{h['rates'][l][1]}" if l in h["rates"] else "-")
                                              for l in langs] for h in hard]
        md += report._table(["Task", "Category"] + [report.display(l) for l in langs], rows)
        md += ["", "First-try passes / runs. A task that is hard in every language may be a badly worded task, one that is "
               "hard only in one language says something about that language.", ""]
    if "nyra" in langs:
        errors = common_nyra_errors(results_list)
        if errors:
            md += ["### Nyra compiler errors on first attempts, all models", ""]
            md += report._table(["Code", "Times", "Example message", "Example task"],
                                [[e["code"], e["count"], _cell(e["message"]), e["task"]] for e in errors]) + [""]
    per_model_failures = {}
    for results in results_list:
        entries = notable_failures(results, failures_per_model)
        per_model_failures[report.model_label(results)] = entries
        if entries:
            md += [f"### Runs that never passed or hit a compiler bug: {report.model_label(results)}", ""]
            md += _failure_table(entries)
    if not any(per_model_failures.values()):
        md += ["### Runs that never passed or hit a compiler bug", "",
               "None: every run passed within the repair budget and no compiler or toolchain bug was hit.", ""]

    md += ["## How to read this", "",
           "- **pass@1**: the share of runs whose first program printed exactly the expected output. **Within N repairs**: "
           "the same when the model may retry up to N times after seeing the compiler's or interpreter's messages or its own "
           "wrong output. Intervals (in the per-model result files) use the number of tasks, not runs, as their sample size.",
           "- **Code tokens**: the program alone, counted with the model's own tokenizer. **Billed output tokens**: what the "
           "API charged for the whole reply, thinking included (many models think before they answer, so this is the larger "
           "number). They answer different questions: how compact is the language, and what did it cost.",
           "- Nyra is given its language spec in the prompt; the other languages rely on what the model already knows. The "
           "tasks are small, input-free programs written by the people who build Nyra. TypeScript is run by Node.js with "
           "the type annotations removed, not type-checked. Rust is compiled with `rustc -O`, edition 2021.",
           "- One run of a model varies from the next; with few samples a difference of a few points is noise.", ""]

    data = {
        "schema": SCHEMA_VERSION, "name": name, "generated": dt.date.today().isoformat(), "mock": mock,
        "complete": complete, "notes": notes,
        "run": {
            "dates": dates, "started_at": started, "finished_at": finished, "langs": langs,
            "tasks": tasks_n, "tasks_sha256": first.get("tasks_sha256"), "samples": first["samples"], "repairs": repairs,
            "timeout_s": first["timeout_s"], "nyra": first.get("nyra"), "spec_sha256": spec.get("sha256"),
            "backend": first.get("backend"), "node": first.get("node"), "rust": first.get("rust"),
            "python": first.get("python"), "harness_commits": commits, "uncommitted_changes": dirty,
        },
        "sources": [{"file": p.name, "sha256": digest} for p, _, digest in loaded],
        "models": {},
        "hardest_tasks": hard, "nyra_compiler_errors": common_nyra_errors(results_list) if "nyra" in langs else [],
    }
    for results in results_list:
        run, summary = results["run"], results["summary"]
        data["models"][report.model_label(results)] = {
            "served_models": run.get("served_models"), "served_by": run.get("served_by"),
            "complete": run.get("complete", True), "spent_usd": run.get("spent_usd"),
            "runs": summary["runs"], "excluded_runs": summary["excluded_runs"], "langs": summary["langs"],
            "paired": summary.get("paired"), "by_category": summary.get("by_category"),
            "notable_failures": per_model_failures[report.model_label(results)],
        }
    return "\n".join(md), data


def publish(paths: list, name: str, out_dir: Path = PUBLISHED_DIR, *, allow_mock: bool = False,
            allow_incomplete: bool = False, allow_mismatch: bool = False, overwrite: bool = False,
            write: bool = True) -> tuple:
    """Build (and, unless write=False, save) the summary. Returns (markdown, data, [paths written])."""
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]*", name):
        raise PublishError(f"--name {name!r}: use letters, digits, '.', '_' and '-' only")
    loaded = load_results(paths)
    results_list = [r for _, r, _ in loaded]
    if any(r["run"].get("mock") for r in results_list) and not allow_mock:
        raise PublishError("refusing to publish a mock run: it replays the reference solutions and measures nothing "
                           "(--allow-mock for a demonstration)")
    notes: list = []
    if any(not r["run"].get("complete", True) for r in results_list):
        if not allow_incomplete:
            raise PublishError("a run was stopped early and covers only part of the tasks; rerun it, or pass "
                               "--allow-incomplete to publish what finished")
        notes.append("at least one run is incomplete (published with --allow-incomplete)")
    problems = check_comparable(results_list)
    if problems:
        if not allow_mismatch:
            raise PublishError("these result files are not comparable: " + "; ".join(problems)
                               + " (--allow-mismatch publishes them anyway)")
        notes += [f"{p} (published with --allow-mismatch)" for p in problems]
    markdown, data = render_publication(name, loaded, notes)
    written: list = []
    if write:
        out_dir.mkdir(parents=True, exist_ok=True)
        md_path, json_path = out_dir / f"{name}.md", out_dir / f"{name}.json"
        if (md_path.exists() or json_path.exists()) and not overwrite:
            raise PublishError(f"{md_path.name} already exists in {out_dir} (published numbers are not replaced "
                               "silently: pick another --name, or pass --overwrite)")
        md_path.write_text(markdown, encoding="utf-8", newline="\n")
        json_path.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n")
        written = [md_path, json_path]
    return markdown, data, written


def main(argv=None) -> int:
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8", errors="replace")
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0], epilog=__doc__.split("\n\n", 1)[1],
                                formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("files", nargs="+", help="raw result files (bench/results/*.json), or a *-compare.json index")
    p.add_argument("--name", required=True, help="name of the summary: bench/published/<name>.md and .json")
    p.add_argument("--out", default=str(PUBLISHED_DIR), help="directory for the summary (default: bench/published)")
    p.add_argument("--stdout", action="store_true", help="print the Markdown instead of writing files (a preview)")
    p.add_argument("--allow-mock", action="store_true", help="accept mock results (they are marked as such)")
    p.add_argument("--allow-incomplete", action="store_true", help="accept runs that were stopped early")
    p.add_argument("--allow-mismatch", action="store_true", help="accept result files measured differently")
    p.add_argument("--overwrite", action="store_true", help="replace an existing summary of the same name")
    args = p.parse_args(argv)
    try:
        markdown, _, written = publish(args.files, args.name, Path(args.out), allow_mock=args.allow_mock,
                                       allow_incomplete=args.allow_incomplete, allow_mismatch=args.allow_mismatch,
                                       overwrite=args.overwrite, write=not args.stdout)
    except PublishError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    if args.stdout:
        print(markdown)
    else:
        print("wrote " + "\n      ".join(str(w) for w in written))
        print("review them, then commit them; the raw result files stay local (bench/results is git-ignored)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
