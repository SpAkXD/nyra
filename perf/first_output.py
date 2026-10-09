#!/usr/bin/env python3
"""Wall-clock time to the first output of `nyra run`, cold (nothing cached) and warm (cached).

    python perf/first_output.py                      # a few bench programs, best of 5
    python perf/first_output.py fizzbuzz big_sieve   # some of them (bench/solutions/nyra/NAME.nyra)
    python perf/first_output.py -n 3 --markdown      # print a Markdown table

For every program it times
  - `nyra run --release X.nyra`   (the generated C is compiled with -O2, what `run` did before v0.6),
  - `nyra run X.nyra`             (the default: -O1),
each with a cold cache (an empty temp folder, so the C compiler really runs) and a warm one
(the executable is already built), and, for comparison, `python X.py` (bench/solutions/python)
and `node X.js` of the program compiled with `nyra build --js`.
The time is the whole command, from its start to its exit, as a user sees it: the best of --runs.
"""

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
EXE = ".exe" if os.name == "nt" else ""
DEFAULT = ["fizzbuzz", "fib_recursive", "word_frequency", "look_and_say", "big_sieve", "text_adventure", "spreadsheet_eval"]


def timed(cmd, env=None):
    t = time.perf_counter()
    p = subprocess.run(cmd, capture_output=True, text=True, env=env)
    return time.perf_counter() - t, p


def cold(cmd, runs, base_env):
    """The best of `runs` runs, each with an empty cache folder for the compiled programs."""
    best = None
    for _ in range(runs):
        with tempfile.TemporaryDirectory(prefix="nyra-cold-") as tmp:
            env = dict(base_env, TEMP=tmp, TMP=tmp, TMPDIR=tmp)
            dt, p = timed(cmd, env)
        if p.returncode != 0:
            sys.exit(f"{cmd} failed:\n{p.stderr}")
        best = dt if best is None else min(best, dt)
    return best


def warm(cmd, runs, base_env):
    with tempfile.TemporaryDirectory(prefix="nyra-warm-") as tmp:
        env = dict(base_env, TEMP=tmp, TMP=tmp, TMPDIR=tmp)
        timed(cmd, env)  # builds and caches the executable
        return min(timed(cmd, env)[0] for _ in range(runs))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("names", nargs="*", help="programs of bench/solutions/nyra (default: a selection)")
    ap.add_argument("-n", "--runs", type=int, default=5)
    ap.add_argument("--nyra", default=str(ROOT / "target" / "release" / f"nyra{EXE}"))
    ap.add_argument("--markdown", action="store_true", help="print a Markdown table")
    args = ap.parse_args()
    names = args.names or DEFAULT
    python = shutil.which("python3") or shutil.which("python")
    node = shutil.which("node")
    env = dict(os.environ)

    rows = []
    for name in names:
        src = ROOT / "bench" / "solutions" / "nyra" / f"{name}.nyra"
        if not src.exists():
            sys.exit(f"no {src}")
        row = {"name": name}
        for label, flags in [("o2", ["--release"]), ("o1", [])]:
            cmd = [args.nyra, "run", str(src), *flags]
            row[f"cold_{label}"] = cold(cmd, args.runs, env)
            row[f"warm_{label}"] = warm(cmd, args.runs, env)
        py = ROOT / "bench" / "solutions" / "python" / f"{name}.py"
        row["python"] = min(timed([python, "-I", str(py)])[0] for _ in range(args.runs)) if python and py.exists() else None
        if node:
            with tempfile.TemporaryDirectory(prefix="nyra-js-") as tmp:
                js = Path(tmp) / f"{name}.js"
                subprocess.run([args.nyra, "build", "--js", str(src), "-o", str(js)], check=True, capture_output=True)
                row["node"] = min(timed([node, str(js)])[0] for _ in range(args.runs))
        else:
            row["node"] = None
        rows.append(row)
        print(f"{name}: done", file=sys.stderr, flush=True)

    def ms(x):
        return "-" if x is None else f"{x * 1000:.0f}"

    head = ["program", "cold -O2 (before)", "cold -O1 (now)", "warm -O2", "warm -O1", "python", "node (--js)"]
    table = [
        [r["name"], ms(r["cold_o2"]), ms(r["cold_o1"]), ms(r["warm_o2"]), ms(r["warm_o1"]), ms(r["python"]), ms(r["node"])] for r in rows
    ]
    if args.markdown:
        print("| " + " | ".join(head) + " |")
        print("|" + "---|" * len(head))
        for t in table:
            print("| " + " | ".join(t) + " |")
    else:
        print("  ".join(f"{h:>18}" if i else f"{h:<16}" for i, h in enumerate(head)))
        for t in table:
            print("  ".join(f"{c:>18}" if i else f"{c:<16}" for i, c in enumerate(t)))


if __name__ == "__main__":
    main()
