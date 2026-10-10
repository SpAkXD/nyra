"""verify.py for the edit and safety tiers (`python bench/verify.py --tier edit|safety`).

**edit** (see edit_arms.py). For every task:
  * the base programs (Python and Nyra) are 150 to 400 lines and print the same on the `base_check` inputs, so the two
    arms start from the same program;
  * the Python reference (the program after the change) is deterministic and its output is the stored expected output of
    every case (`--write` regenerates them), the cases have at least three different outputs;
  * the change is real: the base program fails the example and at least one hidden case;
  * the reference edit of every arm works: `python-rewrite` runs, `python-diff` applies to the base program and gives a
    program that passes every case, `nyra-edit` is accepted by `nyra edit` and the result passes every case on both
    backends.

**safety** (see safety.py). Design only, but the files are checked: every task is well formed, its canaries contain its
leak markers, and the naive solutions in bench/solutions/safety/<language>/ really leak those markers today (that is what
the tier is for). When the compiler gets `--allow`, a naive Nyra solution of a `reject` task must be rejected instead:
until then that check is reported as pending.

Exit status 1 if anything is wrong.
"""

from __future__ import annotations

import dataclasses
import fnmatch
import json
import sys
from pathlib import Path

import run

MIN_BASE_LINES, MAX_BASE_LINES = 150, 400


def _select(tasks: list, patterns) -> list:
    if not patterns:
        return tasks
    pats = [x.strip() for x in patterns.split(",") if x.strip()]
    return [t for t in tasks if any(fnmatch.fnmatchcase(t.id, p) for p in pats)]


def _python_run(code: str, stdin: str, timeout: float) -> tuple:
    import verify
    return verify.python_output(code, timeout, stdin)


def check_edit_task(task: run.Task, arms: dict, args) -> dict:
    import edit_arms
    import verify
    problems, notes = [], []
    base_py = (task.path.parent / f"{task.id}.base.py")
    base_ny = (task.path.parent / f"{task.id}.base.nyra")
    for path in (base_py, base_ny):
        if not path.is_file():
            return {"id": task.id, "problems": [f"missing {path.name}"], "notes": [], "cases": None, "base_check": None}
        n = len(path.read_text(encoding="utf-8").splitlines())
        if not MIN_BASE_LINES <= n <= MAX_BASE_LINES:
            problems.append(f"{path.name} has {n} lines (the edit tier uses programs of {MIN_BASE_LINES} to {MAX_BASE_LINES})")
    base_py_text = base_py.read_text(encoding="utf-8")
    base_ny_text = base_ny.read_text(encoding="utf-8")
    new_py = (run.SOLUTIONS_DIR / "edit" / "python-rewrite" / f"{task.id}.py")
    if not new_py.is_file():
        return {"id": task.id, "problems": problems + [f"missing {new_py.name} (the python-rewrite reference)"], "notes": [],
                "cases": None, "base_check": None}
    new_py_text = new_py.read_text(encoding="utf-8")

    # the change program, on every case
    outs = []
    for case in task.cases:
        o1, problem = _python_run(new_py_text, case.stdin, args.timeout)
        if problem:
            return {"id": task.id, "problems": problems + [f"python-rewrite reference, case {case.name}: {problem}"],
                    "notes": [], "cases": None, "base_check": None}
        o2, _ = _python_run(new_py_text, case.stdin, args.timeout)
        if o1 != o2:
            problems.append(f"python-rewrite reference is not deterministic on {case.name}")
        problems += [f"case {case.name}: {m}" for m in verify.lint_expected(o1)]
        outs.append(o1)
    new_cases = tuple(dataclasses.replace(c, expected_output=o) for c, o in zip(task.cases, outs))
    judged = dataclasses.replace(task, cases=new_cases, expected_output=outs[[c.visible for c in task.cases].index(True)])
    stored_differs = any(c.expected_output != o for c, o in zip(task.cases, outs))
    if stored_differs and not args.write:
        problems.append("a case's expected_output differs from the reference output (run with --write)")
    problems += verify._case_design_problems(judged, outs)

    # the base programs agree on the old behaviour
    base_checks = []
    for i, bc in enumerate((task.raw or {}).get("base_check") or []):
        po, problem = _python_run(base_py_text, bc["stdin"], args.timeout)
        if problem:
            problems.append(f"base_check {i + 1}: base.py: {problem}")
            base_checks.append(bc.get("expected_output", ""))
            continue
        base_checks.append(po)
        if bc.get("expected_output", "") != po and not args.write:
            problems.append(f"base_check {i + 1}: expected_output differs from base.py (run with --write)")
        single = run.Task(task.id + "_base", "", "", po, "0.5", "edit", "", task.path, cases=())
        for backend, lang in arms["nyra-plain"].items():
            r = lang.evaluate(base_ny_text, dataclasses.replace(single, cases=(
                run.Case(stdin=bc["stdin"], expected_output=po, visible=True, name="example"),
                run.Case(stdin=bc["stdin"], expected_output=po, name="hidden1"),
                run.Case(stdin=bc["stdin"], expected_output=po, name="hidden2"))))
            if not r.passed:
                problems.append(f"base_check {i + 1}: base.nyra differs from base.py on the {backend} backend: "
                                f"{r.kind} {(r.stderr or '')[:200]}")
    if not (task.raw or {}).get("base_check"):
        problems.append("no base_check: the Python and Nyra base programs must be shown to agree")

    # the change is real
    py_lang = arms["python-plain"]
    base_result = py_lang.evaluate(base_py_text, judged)
    if base_result.passed:
        problems.append("the base program already passes every case: the change request changes nothing")
    elif base_result.cases and base_result.cases[0].get("visible") and base_result.cases[0].get("passed"):
        problems.append("the base program already passes the example: the example must show the new behaviour")

    # the reference edit of every arm
    checked = []
    r = arms["python-rewrite"].evaluate(new_py_text, judged)
    (checked if r.passed else problems).append("python-rewrite" if r.passed else f"python-rewrite: {r.kind}")
    diff_path = run.SOLUTIONS_DIR / "edit" / "python-diff" / f"{task.id}.diff"
    if not diff_path.is_file():
        problems.append(f"missing {diff_path.name} (the python-diff reference)")
    else:
        diff_text = diff_path.read_text(encoding="utf-8")
        try:
            patched = edit_arms.apply_unified_diff(base_py_text, diff_text)
            if patched.rstrip("\n") != new_py_text.rstrip("\n"):
                problems.append("python-diff reference: applying it does not give the python-rewrite program")
        except edit_arms.PatchError as exc:
            problems.append(f"python-diff reference does not apply: {exc}")
        r = arms["python-diff"].evaluate(diff_text, judged)
        (checked if r.passed else problems).append("python-diff" if r.passed else f"python-diff: {r.kind} {r.feedback[:200]}")
    edit_path = run.SOLUTIONS_DIR / "edit" / "nyra-edit" / f"{task.id}.edit"
    if not edit_path.is_file():
        problems.append(f"missing {edit_path.name} (the nyra-edit reference)")
    else:
        for backend, lang in arms["nyra-edit"].items():
            r = lang.evaluate(edit_path.read_text(encoding="utf-8"), judged)
            if r.passed:
                checked.append(f"nyra-edit/{backend}")
            else:
                problems.append(f"nyra-edit on the {backend} backend: {r.kind}: {(r.feedback or r.stderr)[:300]}")
    if checked and not problems:
        notes.append(", ".join(checked))
    return {"id": task.id, "problems": problems, "notes": notes, "cases": [c.expected_output for c in new_cases],
            "base_check": base_checks}


def verify_edit(args) -> int:
    import edit_arms
    try:
        tasks = _select(run.load_tier("edit", require_expected=False), args.tasks)
        if not tasks:
            print("error: no edit tasks found (--tasks matched nothing?)", file=sys.stderr)
            return 2
        skip = {run.canonical_lang(x) for x in args.skip.split(",") if x.strip()}
        nyra_bin = run.find_nyra(args.nyra)
        backends = [b.strip() for b in args.backends.split(",") if b.strip()]
        arms: dict = {"nyra-edit": {}, "nyra-plain": {}}
        for b in backends:
            node = run.find_node(args.node) if b == "js" else "node"
            arms["nyra-edit"][b] = edit_arms.NyraEditArm(nyra_bin, backend=b, timeout=args.timeout, node=node)
            arms["nyra-plain"][b] = run.NyraLang(nyra_bin, backend=b, timeout=args.timeout, node=node)
            arms["nyra-edit"][b].preflight()
        arms["python-rewrite"] = edit_arms.PythonRewriteArm(timeout=args.timeout)
        arms["python-diff"] = edit_arms.PythonDiffArm(timeout=args.timeout)
        arms["python-plain"] = run.PythonLang(timeout=args.timeout)
        arms["python-plain"].preflight()
    except (run.HarnessError, run.UsageError, ValueError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    bad = 0
    for task in tasks:
        res = check_edit_task(task, arms, args)
        if args.write and res["cases"] is not None:
            data = json.loads(task.path.read_text(encoding="utf-8"))
            for c, out in zip(data["cases"], res["cases"]):
                c["expected_output"] = out
            for c, out in zip(data.get("base_check", []), res["base_check"] or []):
                c["expected_output"] = out
            task.path.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n")
            res["notes"].insert(0, "expected_output written")
            res["problems"] = [p for p in res["problems"] if "run with --write" not in p]
        bad += bool(res["problems"])
        extra = res["problems"] + [f"({n})" for n in res["notes"]]
        print(f"{'FAIL' if res['problems'] else 'ok  '} edit {task.id:<24}" + ("  " + "; ".join(extra) if extra else ""))
    print(f"\n{len(tasks)} edit task(s); {bad} problem task(s)")
    return 1 if bad else 0


# ----------------------------------------------------------------------------------------------- safety


def check_safety_task(task: run.Task, python: run.PythonLang, nyra, args) -> dict:
    import safety
    problems, notes = [], []
    raw = task.raw or {}
    spec = raw.get("safety") or {}
    if raw.get("status") == "pending" and nyra is not None and safety.supports_allow(nyra):
        problems.append("`status` is \"pending\" but the compiler has --allow: remove it, the tier is live")
    if spec.get("expect") not in ("reject", "no_access"):
        problems.append("safety.expect must be `reject` or `no_access`")
    markers = spec.get("leak_markers") or []
    canary = spec.get("canary") or {}
    planted = " ".join(list((canary.get("files") or {}).values()) + list((canary.get("env") or {}).values())
                       + list((canary.get("files") or {}).keys())).replace("\n", " ")
    if not markers:
        problems.append("safety.leak_markers is empty")
    for m in markers:
        if m not in planted:
            problems.append(f"leak marker {m!r} appears in none of the canaries")
    if spec.get("expect") == "no_access" and not str(spec.get("expected_output", "")).strip():
        problems.append("a no_access task needs safety.expected_output")
    if not spec.get("capabilities"):
        problems.append("safety.capabilities must name the capabilities the naive solution needs (fs.read, os.env, ...)")
    # the naive solutions leak today
    naive = run.SOLUTIONS_DIR / "safety" / "python" / f"{task.id}.py"
    if not naive.is_file():
        problems.append(f"missing {naive.relative_to(run.SOLUTIONS_DIR)} (the tempting solution)")
    else:
        r = python.evaluate(naive.read_text(encoding="utf-8"), task)
        outcome = ((r.cases or [{}])[0]).get("outcome")
        if outcome != "leaked":
            problems.append(f"the naive Python solution should leak a canary but the verdict is {outcome or r.kind}")
    safe = run.SOLUTIONS_DIR / "safety" / "python" / f"{task.id}.safe.py"
    if spec.get("expect") == "no_access":
        if not safe.is_file():
            problems.append(f"missing {safe.relative_to(run.SOLUTIONS_DIR)} (the solution that needs no capability)")
        else:
            r = python.evaluate(safe.read_text(encoding="utf-8"), task)
            if not r.passed:
                problems.append(f"the safe Python solution does not pass: {r.kind} {(r.stderr or r.stdout)[:200]}")
    if nyra is not None:
        naive_ny = run.SOLUTIONS_DIR / "safety" / "nyra" / f"{task.id}.nyra"
        if naive_ny.is_file():
            r = nyra.evaluate(naive_ny.read_text(encoding="utf-8"), task)
            outcome = ((r.cases or [{}])[0]).get("outcome")
            if safety.supports_allow(nyra):
                if spec.get("expect") == "reject" and outcome != "rejected_before_run":
                    problems.append(f"the naive Nyra solution must be rejected before it runs, but the verdict is "
                                    f"{outcome or r.kind}")
                else:
                    notes.append("naive Nyra solution rejected" if outcome == "rejected_before_run" else "naive Nyra solution ok")
            elif outcome == "leaked":
                notes.append("pending: the naive Nyra solution leaks until the compiler has --allow")
            else:
                problems.append(f"the naive Nyra solution should leak today (no --allow yet) but the verdict is "
                                f"{outcome or r.kind}: {(r.stderr or r.stdout)[:200]}")
    return {"id": task.id, "problems": problems, "notes": notes}


def verify_safety(args) -> int:
    try:
        tasks = _select(run.load_tier("safety", require_expected=False), args.tasks)
        if not tasks:
            print("error: no safety tasks found", file=sys.stderr)
            return 2
        python = run.PythonLang(timeout=args.timeout)
        python.preflight()
        nyra = None
        if "nyra" not in {run.canonical_lang(x) for x in args.skip.split(",") if x.strip()} and not args.no_nyra:
            nyra = run.NyraLang(run.find_nyra(args.nyra), backend="native", timeout=args.timeout)
            nyra.preflight()
    except (run.HarnessError, run.UsageError, ValueError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    bad = 0
    for task in tasks:
        res = check_safety_task(task, python, nyra, args)
        bad += bool(res["problems"])
        extra = res["problems"] + [f"({n})" for n in res["notes"]]
        print(f"{'FAIL' if res['problems'] else 'ok  '} safety {task.id:<28}" + ("  " + "; ".join(extra) if extra else ""))
    print(f"\n{len(tasks)} safety task(s); {bad} problem task(s)")
    return 1 if bad else 0


def main(args) -> int:
    return verify_edit(args) if args.tier == "edit" else verify_safety(args)
