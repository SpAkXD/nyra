#!/usr/bin/env python3
"""Check the benchmark's reference solutions, and generate the expected outputs.

    python bench/verify.py                 # check everything (needs nyra, node and rustc; see --skip)
    python bench/verify.py --write         # also (re)generate expected_output from the Python references
    python bench/verify.py --tasks fizz*   # only some tasks
    python bench/verify.py --skip rust     # no Rust toolchain here (also: nyra, typescript)
    python bench/verify.py --max-version 0.3   # the compiler implements Nyra 0.3 but its version still says 0.2

What is checked, per task:
  * the task file is well formed, and a Python reference solution exists;
  * the Python reference runs cleanly, twice, with identical output (deterministic), and that output
    is the task's `expected_output`;
  * a TypeScript reference (run by Node) and a Rust reference (compiled by rustc) exist for EVERY task
    and print exactly the expected output, so each expected output is confirmed by independent
    implementations in three languages;
  * if a Nyra reference exists: it produces exactly the same output on BOTH backends (native via C,
    and JavaScript via Node), so the expected output is confirmed by two independent implementations;
  * a task whose min_version the installed compiler already supports has a Nyra reference
    (a warning, or an error with --strict), and a Nyra reference is not older than its min_version.
    "Supports" means the version `nyra --version` prints, or the one given with --max-version.

Exit status 1 if anything is wrong.
"""

from __future__ import annotations

import argparse
import concurrent.futures as cf
import dataclasses
import fnmatch
import json
import sys
from pathlib import Path
from typing import Optional

BENCH_DIR = Path(__file__).resolve().parent
if str(BENCH_DIR) not in sys.path:
    sys.path.insert(0, str(BENCH_DIR))

import run  # noqa: E402


def python_output(code: str, timeout: float, stdin: Optional[str] = None) -> tuple:
    """Run a Python reference solution (on `stdin`, if given). Returns (stdout with LF newlines, problem or None)."""
    with run.scratch_dir() as wd:
        run.write_source(wd / "main.py", code)
        proc = run.run_limited([sys.executable, "-I", "-X", "utf8", "main.py"], cwd=wd, env=run.child_env(wd),
                               timeout=timeout, stdin=None if stdin is None else stdin.encode("utf-8"))
    if proc.spawn_error:
        return "", f"cannot start Python: {proc.spawn_error}"
    if proc.timed_out:
        return "", f"Python reference took longer than {timeout:g} s"
    if proc.returncode != 0:
        return "", "Python reference failed: " + proc.stderr.decode("utf-8", "replace").strip()[-400:]
    return proc.stdout.decode("utf-8", "replace").replace("\r\n", "\n"), None


def lint_expected(text: str) -> list:
    problems = []
    if not text.endswith("\n"):
        problems.append("expected output does not end with a newline")
    if "\r" in text:
        problems.append("expected output contains a carriage return")
    if any(line != line.rstrip() for line in text.split("\n")):
        problems.append("a line of the expected output has trailing whitespace")
    if text.strip() == "":
        problems.append("expected output is empty")
    return problems


def _failure_detail(result: run.EvalResult, expected: str) -> str:
    detail = result.stderr.strip() or result.stdout.strip()
    if result.kind == "compile_error":
        detail = json.dumps(result.errors) if result.errors else (result.stderr or result.feedback).strip()
    elif result.kind == "wrong_output":
        detail = f"first difference on line {run.first_diff_line(run.normalize_output(expected), run.normalize_output(result.stdout))}"
    return f"{result.kind}: {detail[:300]}"


def _case_design_problems(task: run.Task, outs: list) -> list:
    """What makes a task with hidden inputs useless: a program that prints a fixed answer must not get far. So the
    cases have at least three different outputs, and no more than half of the hidden cases may print what the
    example prints (a program that copies the example's output would pass those)."""
    problems = []
    example = next(i for i, c in enumerate(task.cases) if c.visible)
    norm = [run.normalize_output(o) for o in outs]
    if len(set(norm)) < 3:
        problems.append(f"the cases have only {len(set(norm))} different output(s): need at least 3")
    hidden = [i for i in range(len(task.cases)) if i != example]
    same = [task.cases[i].name for i in hidden if norm[i] == norm[example]]
    if len(same) * 2 > len(hidden):
        problems.append(f"too many hidden cases print what the example prints: {', '.join(same)}")
    if len({c.stdin for c in task.cases}) != len(task.cases):
        problems.append("two cases have the same stdin")
    return problems


def check_task(task: run.Task, args, nyra_langs: dict, compiler_version, extra_langs: Optional[dict] = None) -> dict:
    """Returns {"id", "problems": [...], "notes": [...], "new_expected": str|None}.

    `extra_langs` maps a language name (typescript, rust) to its Language: every task must have a
    reference for each of them, whatever its min_version."""
    problems, notes = [], []
    py_ref = run.PythonLang().reference_code(task.id)
    if py_ref is None:
        return {"id": task.id, "problems": ["missing bench/solutions/python/%s.py" % task.id], "notes": [],
                "new_expected": None}

    new_cases = None
    if task.cases:
        # a task with hidden inputs: the Python reference must be deterministic on every case, and each case's
        # stored expected output must be what it prints
        outs, new_cases, any_new = [], [], False
        for case in task.cases:
            o1, problem = python_output(py_ref, args.timeout, case.stdin)
            if problem:
                return {"id": task.id, "problems": [f"case {case.name}: {problem}"], "notes": [], "new_expected": None}
            o2, _ = python_output(py_ref, args.timeout, case.stdin)
            if o1 != o2:
                problems.append(f"Python reference is not deterministic on case {case.name}")
            problems += [f"case {case.name}: {m}" for m in lint_expected(o1)]
            if case.stdin != "" and not case.stdin.endswith("\n"):
                problems.append(f"case {case.name}: stdin does not end with a newline")
            outs.append(o1)
            if run.normalize_output(case.expected_output) != run.normalize_output(o1) or case.expected_output != o1:
                any_new = True
            new_cases.append(dataclasses.replace(case, expected_output=o1))
        out1 = outs[next(i for i, c in enumerate(task.cases) if c.visible)]
        problems += _case_design_problems(task, outs)
        if any_new and not args.write:
            problems.append("a case's expected_output differs from the Python reference output (run with --write)")
        elif any_new:
            notes.append("expected_output written")
    else:
        out1, problem = python_output(py_ref, args.timeout)
        if problem:
            return {"id": task.id, "problems": [problem], "notes": [], "new_expected": None}
        out2, _ = python_output(py_ref, args.timeout)
        if out1 != out2:
            problems.append("Python reference is not deterministic (two runs printed different output)")
        problems += lint_expected(out1)

    stored = task.expected_output
    new_expected = None
    if not task.cases and (run.normalize_output(stored) != run.normalize_output(out1) or stored != out1):
        if args.write:
            new_expected = out1
            notes.append("expected_output written")
        else:
            problems.append("expected_output differs from the Python reference output (run with --write)")

    task_for_nyra = dataclasses.replace(task, expected_output=out1, cases=tuple(new_cases) if new_cases else ())
    checked = []
    if nyra_langs:
        nyra_ref = next(iter(nyra_langs.values())).reference_code(task.id)
        if nyra_ref is None:
            if compiler_version is not None and task.version <= compiler_version:
                msg = f"no Nyra reference solution although the compiler supports Nyra {task.min_version}"
                (problems if args.strict else notes).append(msg)
            else:
                notes.append(f"no Nyra reference yet (needs Nyra {task.min_version})")
        else:
            if compiler_version is not None and task.version > compiler_version:
                problems.append(f"has a Nyra reference but min_version {task.min_version} is above the compiler "
                                f"version {compiler_version[0]}.{compiler_version[1]} (a compiler that already "
                                f"implements Nyra {task.min_version}: --max-version {task.min_version})")
            ok = True
            for backend, lang in nyra_langs.items():
                result = lang.evaluate(nyra_ref, task_for_nyra)
                if not result.passed:
                    ok = False
                    problems.append(f"Nyra reference on the {backend} backend: {_failure_detail(result, out1)}")
            if ok:
                checked.append("nyra: " + " + ".join(nyra_langs))
    for name, lang in (extra_langs or {}).items():
        ref = lang.reference_code(task.id)
        if ref is None:
            if task.tier == "v1":  # the original tasks have all four references; later tiers only the ones that exist
                problems.append(f"missing bench/solutions/{name}/{task.id}{lang.ext}")
            continue
        result = lang.evaluate(ref, task_for_nyra)
        if result.passed:
            checked.append(name)
        else:
            problems.append(f"{lang.display} reference: {_failure_detail(result, out1)}")
    if checked and not problems:
        notes.append(", ".join(checked))
    return {"id": task.id, "problems": problems, "notes": notes, "new_expected": new_expected,
            "new_cases": [c.expected_output for c in new_cases] if (new_cases and args.write) else None}


def main(argv=None) -> int:
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8", errors="replace")
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0],
                                formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--write", action="store_true", help="write expected_output from the Python references")
    p.add_argument("--tier", default="v1", choices=run.TIERS,
                   help="which task set: v1 (the original tasks, default), v2 (stdin and hidden inputs), edit, safety")
    p.add_argument("--tasks", help="comma-separated task ids or patterns (default: all)")
    p.add_argument("--nyra", help="path to the nyra binary (default: target/release, else target/debug)")
    p.add_argument("--no-nyra", action="store_true", help="same as --skip nyra")
    p.add_argument("--skip", default="", help="comma-separated languages whose references are not checked here "
                                              "(nyra, typescript, rust), for a machine without that toolchain")
    p.add_argument("--node", help="the node program that runs TypeScript (default: node on PATH)")
    p.add_argument("--rustc", help="the Rust compiler command, e.g. 'rustc +stable-x86_64-pc-windows-gnu' "
                                   "(default: found automatically)")
    p.add_argument("--backends", default="native,js", help="Nyra backends to check (default: native,js)")
    p.add_argument("--strict", action="store_true", help="a missing Nyra reference for a supported version is an error")
    p.add_argument("--max-version", help="check the Nyra references as if the compiler were this Nyra version, e.g. 0.3 "
                                         "(default: the version `nyra --version` prints; use it while the compiler "
                                         "already implements a language version its version number does not show yet)")
    p.add_argument("--timeout", type=float, default=10.0, help="seconds per program (default: 10)")
    p.add_argument("--jobs", type=int, default=4, help="tasks checked in parallel (default: 4)")
    args = p.parse_args(argv)

    try:
        skip = {run.canonical_lang(x) for x in args.skip.split(",") if x.strip()}
        unknown = skip - set(run.LANG_ORDER)
        if unknown:
            raise run.UsageError(f"--skip: unknown language(s) {', '.join(sorted(unknown))}; "
                                 f"available: {', '.join(run.LANG_ORDER)}")
        if args.no_nyra:
            skip.add("nyra")
        version_limit = run.parse_version(args.max_version) if args.max_version else None
        if args.tier in ("edit", "safety"):
            import verify_tiers  # noqa: E402  (the edit and safety tiers have their own checks)
            return verify_tiers.main(args)
        tasks = run.load_tier(args.tier, require_expected=False)
        if args.tasks:
            patterns = [x.strip() for x in args.tasks.split(",") if x.strip()]
            tasks = [t for t in tasks if any(fnmatch.fnmatchcase(t.id, pat) for pat in patterns)]
            if not tasks:
                print("error: --tasks matched nothing", file=sys.stderr)
                return 2
        nyra_langs: dict = {}
        compiler_version = None
        if "nyra" not in skip:
            for backend in [b.strip() for b in args.backends.split(",") if b.strip()]:
                nyra_langs[backend] = run.NyraLang(run.find_nyra(args.nyra), backend=backend, timeout=args.timeout,
                                                   node=run.find_node(args.node) if backend == "js" else "node")
                nyra_langs[backend].preflight()
            if nyra_langs:
                compiler_version = version_limit or next(iter(nyra_langs.values())).version()
        extra_langs: dict = {}
        if "typescript" not in skip:
            extra_langs["typescript"] = run.TypeScriptLang(args.node, timeout=args.timeout)
        if "rust" not in skip:
            extra_langs["rust"] = run.RustLang(args.rustc, timeout=args.timeout)
        for lang in extra_langs.values():
            lang.preflight()
        run.PythonLang(timeout=args.timeout).preflight()
    except (run.HarnessError, run.UsageError, ValueError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2

    with cf.ThreadPoolExecutor(max_workers=max(1, args.jobs)) as pool:
        results = list(pool.map(lambda t: check_task(t, args, nyra_langs, compiler_version, extra_langs), tasks))

    bad = 0
    for task, res in zip(tasks, results):
        if res["new_expected"] is not None or res.get("new_cases") is not None:
            data = json.loads(task.path.read_text(encoding="utf-8"))
            if res["new_expected"] is not None:
                data["expected_output"] = res["new_expected"]
            if res.get("new_cases") is not None:
                for c, out in zip(data["cases"], res["new_cases"]):
                    c["expected_output"] = out
            task.path.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n")
        status = "FAIL" if res["problems"] else "ok  "
        bad += bool(res["problems"])
        line = f"{status} {task.min_version} {task.id:<24}"
        extra = res["problems"] + [f"({n})" for n in res["notes"]]
        print(line + ("  " + "; ".join(extra) if extra else ""))
    n_nyra = sum(1 for t in tasks if any((run.SOLUTIONS_DIR / sub / "nyra" / f"{t.id}.nyra").is_file()
                                         for sub in (".", "v2")))
    as_version = ""
    if nyra_langs and version_limit:
        as_version = (f"; as Nyra {version_limit[0]}.{version_limit[1]} (--max-version), the compiler says "
                      f"{next(iter(nyra_langs.values())).version_text()}")
    summary = (f"\n{len(tasks)} task(s), {n_nyra} with a Nyra reference"
               + (f" (checked on: {', '.join(nyra_langs)}{as_version})" if nyra_langs else ""))
    for lang in extra_langs.values():
        have = sum(1 for t in tasks if lang.reference_path(t.id).is_file())
        summary += f", {have} with a {lang.display} reference"
    print(summary + f"; {bad} problem task(s)")
    if skip:
        print(f"not checked here (--skip): {', '.join(sorted(skip))}")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
