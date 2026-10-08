#!/usr/bin/env python3
"""Time the reference solutions: the runtime half of the benchmark, with no model and no API key.

    python bench/speed.py                              # the speed tasks, all four languages, 5 timed runs each
    python bench/speed.py --runs 10 --tasks big_sieve,sort_numbers
    python bench/speed.py --all                        # every task (most finish in well under a millisecond)
    python bench/speed.py --langs nyra,python --backend js

For every task and language the reference solution (bench/solutions/) is built once, run once and checked against
the expected output, then run --runs more times, one after the other. Its runtime is the median of those runs minus
the language's start-up time (the median run time of its hello-world program, measured the same way); compile time
is reported separately and not included. Nothing else runs meanwhile, so these numbers are cleaner than the ones a
benchmark run measures next to its parallel jobs. The programs and the timing code are the ones bench/run.py uses
(Nyra: the native executable, or Node.js with --backend js; Python: `python -I`; TypeScript: Node.js; Rust:
`rustc -O`).

The result is printed as Markdown and, unless --no-files, written to bench/results/<date>-speed-references.md and
.json (git-ignored; existing files are never overwritten).
"""

from __future__ import annotations

import argparse
import datetime as dt
import fnmatch
import json
import platform
import sys
from pathlib import Path
from typing import Optional

BENCH_DIR = Path(__file__).resolve().parent
if str(BENCH_DIR) not in sys.path:
    sys.path.insert(0, str(BENCH_DIR))

import report  # noqa: E402
import run  # noqa: E402

SCHEMA_VERSION = 1
DEFAULT_RUNS = 5
DEFAULT_TIMEOUT = 60.0  # seconds per program run: generous, a reference must never be cut short


def select_tasks(tasks: list, patterns: Optional[str], everything: bool) -> list:
    """--tasks patterns (any category), else every task with --all, else the speed tasks."""
    if patterns:
        wanted = [p.strip() for p in patterns.split(",") if p.strip()]
        for pattern in wanted:
            if not any(fnmatch.fnmatchcase(t.id, pattern) for t in tasks):
                raise run.UsageError(f"--tasks: no task matches {pattern!r}")
        return [t for t in tasks if any(fnmatch.fnmatchcase(t.id, p) for p in wanted)]
    if everything:
        return list(tasks)
    return [t for t in tasks if t.category == report.SPEED_CATEGORY]


def measure(tasks: list, langs: dict, lang_names: list, quiet: bool = False) -> dict:
    """{task id: {lang: cell}} where a cell holds the verdict, the runtime and the raw timings of one reference."""
    results: dict = {}
    for task in tasks:
        row = results.setdefault(task.id, {})
        for name in lang_names:
            lang = langs[name]
            code = lang.reference_code(task.id)
            if code is None:
                row[name] = {"passed": None, "kind": "no_reference"}
                continue
            r = lang.evaluate(code, task)
            row[name] = {"passed": r.passed, "kind": r.kind,
                         "runtime_ms": None if r.runtime_ms is None else round(r.runtime_ms, 2),
                         "compile_ms": None if r.compile_ms is None else round(r.compile_ms, 1),
                         "timing": r.timing}
            if not r.passed:
                row[name]["detail"] = (r.stderr or r.stdout or r.feedback).strip()[-300:]
            if not quiet:
                print(f"  {task.id:<22} {lang.display:<11} "
                      + (f"{report.ms(r.runtime_ms)} ms" if r.passed and r.runtime_ms is not None else r.kind),
                      file=sys.stderr, flush=True)
    return results


def summarize(results: dict, lang_names: list) -> dict:
    """Medians over the tasks that every language ran with a runtime (so all languages are compared on the same
    tasks), and each language relative to Python (or to the first language without Python)."""
    complete = [tid for tid, row in results.items()
                if all((row.get(n) or {}).get("runtime_ms") is not None for n in lang_names)]
    ref = report.EFFICIENCY_REFERENCE if report.EFFICIENCY_REFERENCE in lang_names else lang_names[0]
    medians = {n: report.median(results[tid][n]["runtime_ms"] for tid in complete) for n in lang_names}
    compiles = {n: report.median((results[tid].get(n) or {}).get("compile_ms") for tid in results) for n in lang_names}
    return {
        "reference": ref, "tasks": complete,
        "langs": {n: {"median_runtime_ms": medians[n],
                      "runtime_factor": report.factor(medians[n], medians[ref], report.RUNTIME_FLOOR_MS),
                      "median_compile_ms": compiles[n]} for n in lang_names},
    }


def _cell(cell: Optional[dict]) -> str:
    if not cell or cell.get("passed") is None:
        return "-"
    if not cell["passed"]:
        return f"FAIL ({cell['kind']})"
    timing = cell.get("timing") or {}
    if timing.get("error"):
        return f"n/a ({timing['error']})"
    return report.ms(cell.get("runtime_ms"))


def render(data: dict) -> str:
    lang_names = data["langs"]
    names = [report.display(n) for n in lang_names]
    summary = data["summary"]
    ref = report.display(summary["reference"])
    machine = data["machine"]
    tools = []
    if "nyra" in lang_names and machine.get("nyra"):
        tools.append(f"{machine['nyra']} ({data['backend']})")
    if "python" in lang_names:
        tools.append(f"Python {machine['python']}")
    if machine.get("node") and ("typescript" in lang_names or data["backend"] == "js"):
        tools.append(f"Node.js {machine['node'].lstrip('v')}")
    if machine.get("rust") and "rust" in lang_names:
        tools.append(machine["rust"])
    lines = [f"# Reference solutions: runtime, {data['date']}", "",
             f"{', '.join(tools)}; {machine['platform']}. {len(data['tasks'])} task(s); every reference was run once "
             f"and checked, then run {data['runs']} more times one after the other. **Runtime** = the median of those "
             "runs minus the language's start-up time (the median run time of its hello-world program); compile time "
             "is not included. No model is involved: these are the reference solutions in bench/solutions/, which use "
             "the same algorithm in every language, written plainly.", ""]
    rows = [[tid] + [_cell(data["results"][tid].get(n)) for n in lang_names] for tid in data["tasks"]]
    sl = summary["langs"]
    rows.append([f"**Median** ({len(summary['tasks'])} tasks)"] + [f"**{report.ms(sl[n]['median_runtime_ms'])}**"
                                                                   for n in lang_names])
    rows.append([f"**Relative to {ref}**"] + [f"**{report.factor_text(sl[n]['runtime_factor'])}**" for n in lang_names])
    lines += ["## Runtime (ms)", ""] + report._table(["Task"] + names, rows) + [""]
    lines += [f"The median is over the tasks every language ran successfully; relative to {ref} is the median runtime "
              f"divided by {ref}'s (a median below {report.RUNTIME_FLOOR_MS:g} ms counts as "
              f"{report.RUNTIME_FLOOR_MS:g} ms). It is the runtime factor of the efficiency view in the benchmark "
              "reports, which multiplies it by the code-token factor of the models' programs.", ""]
    startup = data["startup_ms"]
    rows = [["Start-up (hello world, median)"] + [report.ms(startup.get(n)) for n in lang_names],
            ["Compile (median over the tasks)"] + [report.ms(sl[n]["median_compile_ms"]) if sl[n]["median_compile_ms"]
                                                   is not None else "-" for n in lang_names]]
    lines += ["## Start-up and compile time (ms)", ""] + report._table(["", *names], rows) + [""]
    lines += ["Compile: `nyra check` + `nyra build` (C compiler included) for Nyra, `rustc -O` for Rust; Python and "
              "TypeScript have no separate step (Node.js strips the types as it loads the file, which counts as "
              "start-up and runtime).", ""]
    return "\n".join(lines)


def main(argv=None) -> int:
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8", errors="replace")
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0], epilog=__doc__.split("\n\n", 1)[1],
                                formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--tasks", help="comma-separated task ids or patterns, any category (default: the speed tasks)")
    p.add_argument("--all", action="store_true", help="every task, not only the speed tasks")
    p.add_argument("--langs", default=",".join(run.LANG_ORDER),
                   help="comma-separated languages (default: %s)" % ",".join(run.LANG_ORDER))
    p.add_argument("--runs", type=int, default=DEFAULT_RUNS,
                   help=f"timed runs per program after the checked run (default: {DEFAULT_RUNS})")
    p.add_argument("--backend", default="native", choices=("native", "js"),
                   help="Nyra backend: the native executable, or JavaScript on Node.js (default: native)")
    p.add_argument("--nyra", help="path to the nyra binary (default: target/release, else target/debug)")
    p.add_argument("--node", help="the node program (default: node on PATH)")
    p.add_argument("--rustc", help="the Rust compiler command (default: found automatically)")
    p.add_argument("--timeout", type=float, default=DEFAULT_TIMEOUT,
                   help=f"seconds per program run (default: {DEFAULT_TIMEOUT:g})")
    p.add_argument("--out", default=str(run.RESULTS_DIR), help="directory for the result files (default: bench/results)")
    p.add_argument("--no-files", action="store_true", help="print the table only, write no files")
    p.add_argument("-q", "--quiet", action="store_true", help="no progress lines")
    args = p.parse_args(argv)

    try:
        lang_names = [run.canonical_lang(n) for n in args.langs.split(",") if n.strip()]
        if not lang_names or len(set(lang_names)) != len(lang_names):
            raise run.UsageError("--langs needs one or more distinct languages, e.g. nyra,python,typescript,rust")
        if args.runs < 1:
            raise run.UsageError("--runs must be at least 1")
        tasks = select_tasks(run.load_tasks(), args.tasks, args.all)
        if not tasks:
            raise run.UsageError("no tasks selected")
        langs = run.make_languages(lang_names, nyra=args.nyra, backend=args.backend, spec=run.DEFAULT_SPEC,
                                   timeout=args.timeout, node=args.node, rustc=args.rustc, time_runs=args.runs)
        for lang in langs.values():
            for warning in lang.preflight():
                if not args.quiet:
                    print(f"warning: {warning}", file=sys.stderr)
    except (run.UsageError, run.HarnessError, ValueError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2

    if not args.quiet:
        print(f"timing {len(tasks)} task(s) x {len(lang_names)} language(s), {args.runs} timed run(s) each",
              file=sys.stderr)
    startup = {n: langs[n].startup_ms() for n in lang_names}
    try:
        results = measure(tasks, langs, lang_names, args.quiet)
    except run.HarnessError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    toolchains = run.toolchain_info(langs)
    data = {
        "schema": SCHEMA_VERSION, "kind": "speed-references", "date": dt.date.today().isoformat(),
        "langs": lang_names, "backend": args.backend, "runs": args.runs, "statistic": "median",
        "machine": {"platform": platform.platform(), "python": platform.python_version(),
                    "node": (toolchains.get("node") or {}).get("version"),
                    "rust": (toolchains.get("rust") or {}).get("version"),
                    "nyra": langs["nyra"].version_text() if "nyra" in langs else None},
        "repo": run._git_info(), "tasks_sha256": run.tasks_digest(), "tasks": [t.id for t in tasks],
        "startup_ms": {n: None if v is None else round(v, 2) for n, v in startup.items()},
        "results": results,
    }
    data["summary"] = summarize(results, lang_names)
    markdown = render(data)
    print(markdown)
    if not args.no_files:
        out_dir = Path(args.out)
        out_dir.mkdir(parents=True, exist_ok=True)
        json_path, md_path = run.result_paths(out_dir, data["date"], "speed", "references")
        json_path.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")
        md_path.write_text(markdown + "\n", encoding="utf-8")
        print(f"wrote {json_path}\n      {md_path}", file=sys.stderr)
    failed = [(tid, n) for tid, row in results.items() for n, cell in row.items() if cell.get("passed") is False]
    if failed:
        print("error: reference solutions failed: " + ", ".join(f"{tid} ({n})" for tid, n in failed), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
