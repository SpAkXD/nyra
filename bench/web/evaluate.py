"""The web A/B test, step 2 of 3: test every site that run_ab.py produced.

    python bench/web/evaluate.py --out bench/web/results/2026-10-19 [--parallel 3] [--perf-runs 3]

Starts serve.py on <out>/sites (and stops it at the end), then for every run with a site:
  check.js  the functional checklist of its brief (checks/<brief>.json) -> <out>/eval/<brief>/<model>/<lang>-<i>.check.json
            and a full-page desktop screenshot                           -> <out>/shots/<brief>/<model>/<lang>-<i>.jpg
  perf.js   median of 3 unthrottled desktop loads (LCP, bytes)           -> <out>/eval/<brief>/<model>/<lang>-<i>.perf.json
Checks run a few at a time; perf runs one site at a time so that the timings do not compete. Resumable.
"""
import argparse
import concurrent.futures as cf
import json
import pathlib
import socket
import subprocess
import sys
import time

HERE = pathlib.Path(__file__).resolve().parent


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def node(args, timeout):
    r = subprocess.run(["node", *args], cwd=HERE, capture_output=True, encoding="utf-8", errors="replace", timeout=timeout)
    if r.returncode != 0 or not r.stdout.strip():
        return {"error": (r.stderr or r.stdout)[-600:]}
    return json.loads(r.stdout.strip().splitlines()[-1])


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    ap.add_argument("--parallel", type=int, default=3)
    ap.add_argument("--perf-runs", type=int, default=3)
    ap.add_argument("--redo", action="store_true", help="test again even when a result exists")
    a = ap.parse_args()
    out = pathlib.Path(a.out).resolve()
    runs = []
    for f in sorted((out / "runs").rglob("*.json")):
        r = json.loads(f.read_text(encoding="utf-8"))
        if r.get("site"):
            runs.append(r)
    port = free_port()
    server = subprocess.Popen([sys.executable, str(HERE / "serve.py"), str(out / "sites"), str(port)])
    time.sleep(1.0)
    try:
        def check(r):
            key = f"{r['brief']}/{r['model']}/{r['lang']}-{r['run']}"
            dest = out / "eval" / r["brief"] / r["model"] / f"{r['lang']}-{r['run']}.check.json"
            if dest.exists() and not a.redo:
                return
            dest.parent.mkdir(parents=True, exist_ok=True)
            shot = out / "shots" / r["brief"] / r["model"] / f"{r['lang']}-{r['run']}"
            shot.parent.mkdir(parents=True, exist_ok=True)
            url = f"http://127.0.0.1:{port}/{r['site'][len('sites/'):]}/"
            res = None
            for attempt in range(2):  # a checker crash (not a failed item) is retried once
                res = node(["check.js", url, str(HERE / "checks" / f"{r['brief']}.json"), str(shot)], 600)
                if "error" not in res:
                    break
            dest.write_text(json.dumps(res, indent=1, ensure_ascii=False), encoding="utf-8")
            print(f"check {key}: {res.get('passed')}/{res.get('total')}" + (f" ERROR {res['error'][:200]}" if "error" in res else ""), flush=True)

        with cf.ThreadPoolExecutor(a.parallel) as ex:
            list(ex.map(check, runs))
        for r in runs:
            dest = out / "eval" / r["brief"] / r["model"] / f"{r['lang']}-{r['run']}.perf.json"
            if dest.exists() and not a.redo:
                continue
            url = f"http://127.0.0.1:{port}/{r['site'][len('sites/'):]}/"
            res = node(["perf.js", url, str(a.perf_runs)], 600)
            dest.write_text(json.dumps(res, indent=1), encoding="utf-8")
            print(f"perf {r['brief']}/{r['model']}/{r['lang']}-{r['run']}: LCP {res.get('lcp_ms')} ms, {res.get('kb')} KB", flush=True)
    finally:
        server.terminate()
        server.wait(10)


if __name__ == "__main__":
    main()
