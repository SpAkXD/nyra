#!/usr/bin/env python3
"""Wall-clock time to the first output of `nyra run`, before (native) and after (auto), cold and warm.

    python perf/first_output.py                      # a selection of bench programs, best of 5
    python perf/first_output.py fizzbuzz big_sieve   # some of them (bench/solutions/nyra/NAME.nyra)
    python perf/first_output.py v2/bracket_check     # a v2 task (bench/solutions/v2/nyra), fed its example input
    python perf/first_output.py -n 3 --markdown      # print a Markdown table
    python perf/first_output.py --steps 8000000      # try another budget of the interpreter (NYRA_AUTO_STEPS)

For every program it times
  - `nyra run --native X.nyra`   (what `run` did before v0.7: always the C compiler), cold: an empty temp
                                 folder, so the C compiler really runs; and warm: the executable is cached,
  - `nyra run X.nyra`            (the default, auto mode: the interpreter first), cold,
and, for comparison, `python X.py` (bench/solutions/python) and `node X.js` of the program compiled with
`nyra build --js`. `how` says what the auto run did: `interp` (the interpreter finished it) or `native`
(it ran out of its budget and ran natively). The time is the whole command, from its start to its exit,
as a user sees it: the best of --runs.
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
EXE = ".exe" if os.name == "nt" else ""
DEFAULT = [
    "fizzbuzz",
    "fib_recursive",
    "word_frequency",
    "look_and_say",
    "big_sieve",
    "text_adventure",
    "spreadsheet_eval",
    "bank_ledger",
    "binary_search",
    "ackermann",
    "v2/bracket_check",
    "v2/edit_distance",
    "v2/knapsack_best",
    "v2/life_generations",
]


def timed(cmd, env=None, stdin=None):
    t = time.perf_counter()
    p = subprocess.run(cmd, capture_output=True, input=stdin, env=env)
    return time.perf_counter() - t, p


def best(cmd, runs, base_env, stdin, cold):
    """The best of `runs` runs. Cold: each run has an empty cache folder for the compiled programs.
    Warm: one run builds and caches the executable first. Returns (seconds, last process)."""
    best_t, last = None, None
    warm_tmp = None if cold else tempfile.mkdtemp(prefix="nyra-warm-")
    try:
        if not cold:
            env = dict(base_env, TEMP=warm_tmp, TMP=warm_tmp, TMPDIR=warm_tmp)
            timed(cmd, env, stdin)
        for _ in range(runs):
            if cold:
                with tempfile.TemporaryDirectory(prefix="nyra-cold-") as tmp:
                    env = dict(base_env, TEMP=tmp, TMP=tmp, TMPDIR=tmp)
                    dt, p = timed(cmd, env, stdin)
            else:
                env = dict(base_env, TEMP=warm_tmp, TMP=warm_tmp, TMPDIR=warm_tmp)
                dt, p = timed(cmd, env, stdin)
            if p.returncode != 0:
                sys.exit(f"{cmd} failed ({p.returncode}):\n{p.stderr.decode(errors='replace')}")
            best_t = dt if best_t is None else min(best_t, dt)
            last = p
    finally:
        if warm_tmp:
            shutil.rmtree(warm_tmp, ignore_errors=True)
    return best_t, last


def load(name):
    """(label, nyra source, python source or None, stdin bytes) of a program name."""
    if name.startswith("v2/"):
        stem = name[3:]
        src = ROOT / "bench" / "solutions" / "v2" / "nyra" / f"{stem}.nyra"
        py = ROOT / "bench" / "solutions" / "v2" / "python" / f"{stem}.py"
        task = json.loads((ROOT / "bench" / "tasks" / "v2" / f"{stem}.json").read_text(encoding="utf-8"))
        case = next(c for c in task["cases"] if c.get("visible"))
        return name, src, py, case["stdin"].encode()
    src = ROOT / "bench" / "solutions" / "nyra" / f"{name}.nyra"
    py = ROOT / "bench" / "solutions" / "python" / f"{name}.py"
    return name, src, py, b""


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("names", nargs="*", help="programs of bench/solutions/nyra, or v2/NAME (default: a selection)")
    ap.add_argument("-n", "--runs", type=int, default=5)
    ap.add_argument("--nyra", default=str(ROOT / "target" / "release" / f"nyra{EXE}"))
    ap.add_argument("--steps", type=int, help="the interpreter's budget of steps (NYRA_AUTO_STEPS)")
    ap.add_argument("--markdown", action="store_true", help="print a Markdown table")
    args = ap.parse_args()
    names = args.names or DEFAULT
    python = shutil.which("python3") or shutil.which("python")
    node = shutil.which("node")
    env = dict(os.environ)
    if args.steps:
        env["NYRA_AUTO_STEPS"] = str(args.steps)

    rows = []
    for name in names:
        label, src, py, stdin = load(name)
        if not src.exists():
            sys.exit(f"no {src}")
        row = {"name": label}
        row["cold_native"], _ = best([args.nyra, "run", "--native", str(src)], args.runs, env, stdin, cold=True)
        row["warm_native"], _ = best([args.nyra, "run", "--native", str(src)], args.runs, env, stdin, cold=False)
        row["cold_auto"], p = best([args.nyra, "run", str(src)], args.runs, env, stdin, cold=True)
        # what the auto run did, from `--time`
        _, q = timed([args.nyra, "run", "--time", str(src)], dict(env), stdin)
        err = q.stderr.decode(errors="replace")
        steps = re.search(r"\((\d+) steps\)", err)
        row["how"] = "interp" if "interpreted" in err else "native"
        row["steps"] = int(steps.group(1)) if steps else None
        row["python"] = (
            min(timed([python, "-I", str(py)], None, stdin)[0] for _ in range(args.runs)) if python and py and py.exists() else None
        )
        if node:
            with tempfile.TemporaryDirectory(prefix="nyra-js-") as tmp:
                js = Path(tmp) / "prog.js"
                subprocess.run([args.nyra, "build", "--js", str(src), "-o", str(js)], check=True, capture_output=True)
                row["node"] = min(timed([node, str(js)], None, stdin)[0] for _ in range(args.runs))
        else:
            row["node"] = None
        rows.append(row)
        print(f"{label}: done", file=sys.stderr, flush=True)

    def ms(x):
        return "-" if x is None else f"{x * 1000:.0f}"

    head = ["program", "before: cold --native", "after: cold auto", "how", "steps", "warm native", "python", "node (--js)"]
    table = [
        [r["name"], ms(r["cold_native"]), ms(r["cold_auto"]), r["how"], "-" if r["steps"] is None else f"{r['steps']:,}", ms(r["warm_native"]), ms(r["python"]), ms(r["node"])]
        for r in rows
    ]
    if args.markdown:
        print("| " + " | ".join(head) + " |")
        print("|" + "---|" * len(head))
        for t in table:
            print("| " + " | ".join(t) + " |")
    else:
        print("  ".join(f"{h:>22}" if i else f"{h:<22}" for i, h in enumerate(head)))
        for t in table:
            print("  ".join(f"{c:>22}" if i else f"{c:<22}" for i, c in enumerate(t)))


if __name__ == "__main__":
    main()
