#!/usr/bin/env python3
"""Times the native Nyra programs in perf/ against hand-written C, Rust, Go, Node.js and Python.

    python perf/run.py                 # every benchmark, best of 5 runs
    python perf/run.py sieve fib -n 3  # some of them
    python perf/run.py --json out.json # also write the timings as JSON
    python perf/run.py --python-runs 3 # Python is slow: it runs once unless asked

Each `perf/X.nyra` is built with `nyra build` (native, optimized), `perf/c/X.c` with the same C
compiler and flags nyra uses, `perf/rs/X.rs` with `rustc -C opt-level=3`, `perf/go/X.go` with
`go build`; `perf/js/X.js` runs on `node` and `perf/py/X.py` on `python`. All of them must print
the same output. The time is the best wall-clock time of the executable itself (process start
included, so a few milliseconds are noise; Node's start is about 40 ms).

Needs a built nyra (`cargo build --release`; `--nyra PATH` picks another). The others are skipped
when missing: a C compiler (NYRA_CC, else the one nyra would pick), rustc (NYRA_RUSTC), go
(NYRA_GO), node (NYRA_NODE) and python (NYRA_PYTHON, else python3 or python).
"""

import argparse
import json
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
# the flags nyra passes to the C compiler (src/main.rs CC_FLAGS)
CC_FLAGS = ["-O2", "-fwrapv", "-ffp-contract=off"] + ([] if sys.platform == "darwin" else ["-s"])


def find_cc():
    if os.environ.get("NYRA_CC"):
        return os.environ["NYRA_CC"]
    if os.name == "nt":
        for cc in [r"C:\msys64\ucrt64\bin\gcc.exe", r"C:\msys64\mingw64\bin\gcc.exe", r"C:\msys64\clang64\bin\clang.exe"]:
            if Path(cc).exists():
                return cc
    for cc in ["gcc", "clang", "cc"]:
        if shutil.which(cc):
            return cc
    return None


def tool_env(tool):
    """A compiler given by full path needs its own directory on PATH (DLLs, as and ld)."""
    env = dict(os.environ)
    d = Path(tool).parent
    if str(d) not in ("", "."):
        env["PATH"] = str(d) + os.pathsep + env.get("PATH", "")
    return env


def find_tool(env, names):
    """The tool named by the environment variable `env`, else the first of `names` on PATH."""
    if os.environ.get(env):
        return os.environ[env]
    for n in names:
        if shutil.which(n):
            return n
    return None


def build(name, out, nyra, cc, rustc, go, node, python):
    """Builds the executables of a benchmark; returns {label: command (a list)}."""
    exes = {}
    src = HERE / f"{name}.nyra"
    exe = out / f"{name}-nyra{EXE}"
    subprocess.run([nyra, "build", str(src), "-o", str(exe)], check=True, stderr=subprocess.DEVNULL)
    exes["nyra"] = [str(exe)]
    c = HERE / "c" / f"{name}.c"
    if cc and c.exists():
        exe = out / f"{name}-c{EXE}"
        subprocess.run([cc, *CC_FLAGS, "-o", str(exe), str(c)], check=True, env=tool_env(cc))
        exes["c"] = [str(exe)]
    r = HERE / "rs" / f"{name}.rs"
    if rustc and r.exists():
        exe = out / f"{name}-rs{EXE}"
        subprocess.run([rustc, "--edition", "2021", "-C", "opt-level=3", "-C", "debuginfo=0", "-o", str(exe), str(r)], check=True)
        exes["rust"] = [str(exe)]
    g = HERE / "go" / f"{name}.go"
    if go and g.exists():
        exe = out / f"{name}-go{EXE}"
        subprocess.run([go, "build", "-o", str(exe), str(g)], check=True)
        exes["go"] = [str(exe)]
    j = HERE / "js" / f"{name}.js"
    if node and j.exists():
        exes["node"] = [node, str(j)]
    y = HERE / "py" / f"{name}.py"
    if python and y.exists():
        exes["python"] = [python, str(y)]
    return exes


def best_time(cmd, runs):
    """The fastest of `runs` runs of the command, and the output (which must not change between runs)."""
    best, output = None, None
    exe = cmd[-1]
    for _ in range(runs):
        t = time.perf_counter()
        p = subprocess.run(cmd, capture_output=True, text=True)
        dt = time.perf_counter() - t
        if p.returncode != 0:
            sys.exit(f"{exe} failed with exit code {p.returncode}:\n{p.stderr}")
        out = p.stdout.replace("\r\n", "\n")
        if output is not None and out != output:
            sys.exit(f"{exe}: the output changed between runs")
        output = out
        best = dt if best is None else min(best, dt)
    return best, output


LABELS = ["nyra", "c", "rust", "go", "node", "python"]


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("names", nargs="*", help="benchmarks to run (default: all perf/*.nyra)")
    ap.add_argument("-n", "--runs", type=int, default=5, help="runs per program; the best counts (default 5)")
    ap.add_argument("--python-runs", type=int, default=1, help="runs of each Python program (default 1: they are slow)")
    ap.add_argument("--nyra", default=str(ROOT / "target" / "release" / f"nyra{EXE}"), help="the nyra executable")
    ap.add_argument("--json", help="also write the results to this file")
    args = ap.parse_args()

    names = args.names or sorted(p.stem for p in HERE.glob("*.nyra"))
    if not Path(args.nyra).exists():
        sys.exit(f"no nyra at {args.nyra}: run `cargo build --release` first (or pass --nyra)")
    cc = find_cc()
    rustc = os.environ.get("NYRA_RUSTC") or shutil.which("rustc")
    go = find_tool("NYRA_GO", ["go"])
    node = find_tool("NYRA_NODE", ["node"])
    python = find_tool("NYRA_PYTHON", ["python3", "python"])
    for label, tool in [("C", cc), ("Rust", rustc), ("Go", go), ("Node.js", node), ("Python", python)]:
        if not tool:
            print(f"no {label} found: its column is skipped", file=sys.stderr)

    results = {}
    shown = ["nyra", "c", "rust", "go", "node", "python"]
    with tempfile.TemporaryDirectory(prefix="nyra-perf-") as tmp:
        out = Path(tmp)
        print(f"{'benchmark':<10}" + "".join(f" {label:>9}" for label in shown) + " " + "".join(f" {'nyra/' + label:>11}" for label in shown[1:]))
        for name in names:
            exes = build(name, out, args.nyra, cc, rustc, go, node, python)
            times, outputs = {}, {}
            for label, cmd in exes.items():
                times[label], outputs[label] = best_time(cmd, args.python_runs if label == "python" else args.runs)
            for label, o in outputs.items():
                if o != outputs["nyra"]:
                    sys.exit(f"{name}: {label} printed:\n{o}\nbut nyra printed:\n{outputs['nyra']}")
            results[name] = times

            def col(label):
                return f"{times[label]:8.3f}s" if label in times else f"{'-':>9}"

            def ratio(label):
                return f"{times['nyra'] / times[label]:10.2f}x" if label in times else f"{'-':>11}"

            print(f"{name:<10}" + "".join(f" {col(label)}" for label in shown) + " " + "".join(f" {ratio(label)}" for label in shown[1:]), flush=True)
    if args.json:
        Path(args.json).write_text(json.dumps(results, indent=2) + "\n")


if __name__ == "__main__":
    main()
