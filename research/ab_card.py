"""Summarize the card A/B runs (research/AB-card.md): one row per result file, and paired comparisons.

    python research/ab_card.py LABEL=bench/results/FILE.json [LABEL=FILE.json ...] [--pair A:B ...]

Each LABEL is an arm name (for example `sonnet-full`). Prints Markdown: pass rates (all tasks, the hard tier, the
other tasks), pass within repairs, tokens, cost (what the API was charged for the run, the cache warm-up included) and
the prompt-cache numbers, then for every --pair A:B the tasks on which the first try of one arm passed and the other's
did not, with the exact two-sided sign test. No network, no key.
"""
import argparse
import json
import statistics
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "bench"))
import report  # noqa: E402


def load(path: str) -> dict:
    return json.loads(Path(path).read_text(encoding="utf-8"))


def attempts(data):
    return [a for r in data["records"] for a in r["attempts"]]


def pct(k, n):
    return f"{k}/{n} ({k / n:.0%})" if n else "-"


def row(label: str, data: dict) -> list:
    cat = {tid: t["category"] for tid, t in data["run"]["tasks"].items()}
    recs = [r for r in data["records"] if r["status"] in ("pass", "fail")]
    hard = [r for r in recs if cat[r["task_id"]] == "hard"]
    rest = [r for r in recs if cat[r["task_id"]] != "hard"]
    ft = lambda rs: sum(1 for r in rs if r["first_try"])  # noqa: E731
    wr = lambda rs: sum(1 for r in rs if r["status"] == "pass")  # noqa: E731
    first = [r["attempts"][0] for r in recs if r["attempts"]]
    every = attempts(data)
    u = lambda a, k: (a.get("usage") or {}).get(k) or 0  # noqa: E731
    warm = data["run"].get("cache", {}).get("warmup", [])
    read = sum(u(a, "cache_read_input_tokens") for a in every) + sum((w["usage"].get("cache_read_input_tokens") or 0) for w in warm)
    written = sum(u(a, "cache_creation_input_tokens") for a in every) + sum((w["usage"].get("cache_creation_input_tokens") or 0) for w in warm)
    inputs = sum(u(a, "input_tokens") for a in every) + sum((w["usage"].get("input_tokens") or 0) for w in warm)
    code = [a["code_tokens"] for a in first if a.get("code_tokens")]
    errors = sum(1 for r in data["records"] if r["status"] not in ("pass", "fail"))
    return [label, pct(ft(recs), len(recs)), pct(ft(hard), len(hard)), pct(ft(rest), len(rest)),
            pct(wr(recs), len(recs)),
            f"{statistics.mean(u(a, 'output_tokens') for a in first):,.0f}" if first else "-",
            f"{statistics.median(code):,.0f}" if code else "-",
            f"{statistics.mean(u(a, 'input_tokens') for a in every):,.0f}" if every else "-",
            f"${data['run']['spent_usd']:.3f}" if data["run"].get("spent_usd") is not None else "-",
            f"{read:,} / {written:,} ({read / inputs:.0%})" if inputs else "-",
            f"{errors}"]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("arms", nargs="+", help="LABEL=result.json")
    ap.add_argument("--pair", action="append", default=[], help="A:B (labels): first-try comparison of two arms")
    args = ap.parse_args()
    arms = {}
    for item in args.arms:
        label, _, path = item.partition("=")
        arms[label] = load(path)
    head = ["arm", "pass@1", "hard", "other", "within repairs", "out tok / first try", "code tok (median)",
            "input tok / attempt", "cost", "cache read / written (share of input)", "errors"]
    print("| " + " | ".join(head) + " |")
    print("|" + "---|" * len(head))
    for label, data in arms.items():
        print("| " + " | ".join(row(label, data)) + " |")
    for pair in args.pair:
        a, b = pair.split(":")
        fa = {r["task_id"]: r["first_try"] for r in arms[a]["records"] if r["status"] in ("pass", "fail")}
        fb = {r["task_id"]: r["first_try"] for r in arms[b]["records"] if r["status"] in ("pass", "fail")}
        common = sorted(set(fa) & set(fb))
        only_a = [t for t in common if fa[t] and not fb[t]]
        only_b = [t for t in common if fb[t] and not fa[t]]
        p = report.mcnemar_exact(len(only_a), len(only_b)) if only_a or only_b else 1.0
        print(f"\n{a} vs {b}: {len(common)} tasks in common; first try passed only by {a}: {len(only_a)}"
              f"{' (' + ', '.join(only_a) + ')' if only_a else ''}; only by {b}: {len(only_b)}"
              f"{' (' + ', '.join(only_b) + ')' if only_b else ''}; exact sign test p = {p:.2f}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
