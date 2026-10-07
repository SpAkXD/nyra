"""Metrics and the Markdown summary of a benchmark run (pure functions, standard library only).

Metrics are computed on the runs for which *every* language produced a valid result (status
pass or fail). A run that ended in an API error is dropped for all languages, so the languages
are always compared on exactly the same tasks.
"""

from __future__ import annotations

import math
from collections import Counter
from typing import Optional

FAILURE_KINDS = ("no_code", "compile_error", "toolchain_error", "runtime_error", "timeout", "output_limit",
                 "wrong_output")
DISPLAY = {"nyra": "Nyra", "python": "Python"}


# ---------------------------------------------------------------------------- statistics


def mean(values) -> Optional[float]:
    values = [v for v in values if v is not None]
    return sum(values) / len(values) if values else None


def wilson_p(p: float, n: float, z: float = 1.96) -> tuple:
    """95% Wilson score interval for a proportion p measured on n trials (sensible for small n and for 0%/100%)."""
    if n <= 0:
        return (0.0, 0.0)
    denom = 1 + z * z / n
    centre = (p + z * z / (2 * n)) / denom
    half = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / denom
    return (max(0.0, centre - half), min(1.0, centre + half))


def wilson(k: int, n: int, z: float = 1.96) -> tuple:
    """Wilson interval for k successes in n trials."""
    return wilson_p(k / n if n else 0.0, n, z)


def mcnemar_exact(only_a: int, only_b: int) -> float:
    """Two-sided exact McNemar test: are the two languages' disagreements balanced?
    only_a / only_b = tasks that only A / only B got right."""
    n = only_a + only_b
    if n == 0:
        return 1.0
    k = min(only_a, only_b)
    tail = sum(math.comb(n, i) for i in range(k + 1)) / 2 ** n
    return min(1.0, 2 * tail)


def paired_sign_test(diffs: list, rounds: int = 20000, seed: int = 20261007) -> float:
    """Two-sided test that paired per-task differences are balanced around zero.

    `diffs` holds one difference per task (language A's first-try rate minus language B's). With a single
    sample per task every non-zero difference is +-1 and this is exactly McNemar's exact test; with several
    samples the differences are fractions and the p-value comes from a seeded random sign-flip test. Tasks,
    not repeated runs, are the unit, so repeating a task does not make the evidence look stronger.
    """
    nz = [d for d in diffs if abs(d) > 1e-12]
    if not nz:
        return 1.0
    if max(abs(d) for d in nz) - min(abs(d) for d in nz) < 1e-12:
        pos = sum(1 for d in nz if d > 0)
        return mcnemar_exact(pos, len(nz) - pos)
    import random
    rng = random.Random(seed)
    observed = abs(sum(nz))
    hits = sum(1 for _ in range(rounds) if abs(sum(d if rng.random() < 0.5 else -d for d in nz)) >= observed - 1e-9)
    return (hits + 1) / (rounds + 1)


# ---------------------------------------------------------------------------- aggregation


def _valid(rec) -> bool:
    return rec is not None and rec["status"] in ("pass", "fail")


def _usage(attempt: dict, field: str):
    return (attempt.get("usage") or {}).get(field)


def _lang_stats(recs: list) -> dict:
    n = len(recs)
    first = [r["attempts"][0] for r in recs if r["attempts"]]
    every = [a for r in recs for a in r["attempts"]]
    k1 = sum(1 for r in recs if r["first_try"])
    kn = sum(1 for r in recs if r["status"] == "pass")
    totals_per_run = []
    for r in recs:
        outs = [_usage(a, "output_tokens") for a in r["attempts"]]
        if outs and all(o is not None for o in outs):
            totals_per_run.append(sum(outs))
    fail_kinds = Counter(a["result"]["kind"] for a in first if not a["result"]["passed"])
    codes = Counter(e.get("code", "?") for a in first for e in a["result"].get("errors", []))
    n_tasks = len({r["task_id"] for r in recs})  # the interval's sample size: tasks, not repeated runs of them
    return {
        "n": n, "tasks": n_tasks,
        "pass_at_1": k1, "pass_at_1_rate": k1 / n if n else None,
        "pass_at_1_ci": list(wilson_p(k1 / n, n_tasks)) if n else [0.0, 0.0],
        "pass_within_repairs": kn, "pass_within_repairs_rate": kn / n if n else None,
        "pass_within_repairs_ci": list(wilson_p(kn / n, n_tasks)) if n else [0.0, 0.0],
        "avg_attempts": mean(r["attempts_used"] for r in recs),
        "avg_output_tokens_per_attempt": mean(_usage(a, "output_tokens") for a in every),
        "avg_output_tokens_first_attempt": mean(_usage(a, "output_tokens") for a in first),
        "avg_output_tokens_per_run": mean(totals_per_run),
        "avg_input_tokens_per_attempt": mean(_usage(a, "input_tokens") for a in every),
        "avg_input_tokens_first_attempt": mean(_usage(a, "input_tokens") for a in first),
        "avg_code_tokens_first_attempt": mean(a.get("code_tokens") for a in first),
        "avg_chars_first_attempt": mean(a.get("chars") for a in first),
        "avg_lines_first_attempt": mean(a.get("lines") for a in first),
        "total_input_tokens": sum(_usage(a, "input_tokens") or 0 for a in every),
        "total_output_tokens": sum(_usage(a, "output_tokens") or 0 for a in every),
        "attempts_total": len(every),
        "odd_stop_reasons": dict(Counter(a["stop_reason"] for a in every if a.get("stop_reason") not in (None, "end_turn"))),
        "first_attempt_failures": dict(fail_kinds),
        "first_attempt_error_codes": dict(codes),
        "toolchain_errors": sum(1 for a in every if a["result"]["kind"] == "toolchain_error"),
    }


def summarize(records: list, langs: list) -> dict:
    by_key: dict = {}
    for r in records:
        by_key.setdefault((r["task_id"], r["sample"]), {})[r["lang"]] = r
    keys = sorted(k for k, d in by_key.items() if all(_valid(d.get(lang)) for lang in langs))
    out: dict = {
        "tasks": len({k[0] for k in keys}), "runs": len(keys), "excluded_runs": len(by_key) - len(keys),
        "langs": {lang: _lang_stats([by_key[k][lang] for k in keys]) for lang in langs}, "paired": None,
    }
    if keys:
        both_first = [k for k in keys if all(by_key[k][lang]["first_try"] for lang in langs)]
        paired: dict = {"n": len(both_first), "langs": {}}
        for lang in langs:
            firsts = [by_key[k][lang]["attempts"][0] for k in both_first]
            paired["langs"][lang] = {
                "avg_output_tokens": mean(_usage(a, "output_tokens") for a in firsts),
                "avg_code_tokens": mean(a.get("code_tokens") for a in firsts),
                "avg_chars": mean(a.get("chars") for a in firsts),
                "avg_lines": mean(a.get("lines") for a in firsts),
            }
        if len(langs) == 2:
            a, b = langs
            only_a = sum(1 for k in keys if by_key[k][a]["first_try"] and not by_key[k][b]["first_try"])
            only_b = sum(1 for k in keys if by_key[k][b]["first_try"] and not by_key[k][a]["first_try"])
            neither = sum(1 for k in keys if not by_key[k][a]["first_try"] and not by_key[k][b]["first_try"])
            per_task = {}  # task -> [first-try passes of a, of b, runs]
            for k in keys:
                row = per_task.setdefault(k[0], [0, 0, 0])
                row[0] += by_key[k][a]["first_try"]
                row[1] += by_key[k][b]["first_try"]
                row[2] += 1
            diffs = [(ra - rb) / runs for ra, rb, runs in per_task.values()]
            paired["first_try"] = {"both": len(both_first), f"only_{a}": only_a, f"only_{b}": only_b,
                                   "neither": neither, "sign_test_p": paired_sign_test(diffs)}
        out["paired"] = paired
    return out


# ------------------------------------------------------------------------------ rendering


def _pct(rate: Optional[float]) -> str:
    return "n/a" if rate is None else f"{100 * rate:.0f}%"


def _rate_cell(k: int, n: int, ci) -> str:
    if n == 0:
        return "n/a"
    return f"{_pct(k / n)} ({k}/{n}) [{100 * ci[0]:.0f}-{100 * ci[1]:.0f}%]"


def _num(value: Optional[float], digits: int = 0) -> str:
    return "n/a" if value is None else f"{value:,.{digits}f}"


def _ratio(a: Optional[float], b: Optional[float]) -> str:
    return "n/a" if not a or not b else f"{a / b:.2f}x"


def _table(header: list, rows: list) -> list:
    lines = ["| " + " | ".join(header) + " |", "|" + "|".join("---" for _ in header) + "|"]
    lines += ["| " + " | ".join(str(c) for c in row) + " |" for row in rows]
    return lines


def _task_cell(recs: list, repairs: int) -> str:
    if not recs:
        return "-"
    if len(recs) == 1:
        r = recs[0]
        if r["status"] == "pass":
            return "pass" if r["attempts_used"] == 1 else f"pass (attempt {r['attempts_used']})"
        if r["status"] == "fail":
            last = r["attempts"][-1]["result"] if r["attempts"] else {}
            detail = last.get("kind", "?")
            codes = [e.get("code") for e in last.get("errors", []) if e.get("code")]
            return f"FAIL ({detail}{' ' + codes[0] if codes else ''})"
        return r["status"]
    valid = [r for r in recs if r["status"] in ("pass", "fail")]
    k1 = sum(1 for r in valid if r["first_try"])
    kn = sum(1 for r in valid if r["status"] == "pass")
    return f"{k1}/{len(valid)} first try, {kn}/{len(valid)} within {repairs} repairs"


def _group_excluded(excluded: list) -> str:
    groups: dict = {}
    for e in excluded:
        groups.setdefault(e["reason"], []).append(e["id"])
    parts = []
    for reason, ids in groups.items():
        listed = ", ".join(ids) if len(ids) <= 6 else f"{len(ids)} tasks"
        parts.append(f"{reason}: {listed}")
    return "; ".join(parts)


def render_markdown(results: dict, tasks_by_id: dict) -> str:
    run, summary = results["run"], results["summary"]
    langs = run["langs"]
    repairs = run["repairs"]
    est = " (estimated)" if run.get("tokens_are_estimates") else ""
    prov = run["provider"]
    lines = [f"# Nyra benchmark: {prov['model']} ({prov['name']}), {run['date']}", ""]
    if run.get("mock"):
        lines += ["> **MOCK RUN: this is a self-test of the pipeline, not a measurement.** The \"model\" replays the "
                  "reference solutions, so tasks pass by construction"
                  + (" (apart from the deliberately broken first attempts of --mock-flaky)" if run.get("mock_flaky") else "")
                  + ", and token counts are character-based estimates.", ""]
    if not run.get("complete", True):
        lines += ["> **INCOMPLETE RUN:** it was stopped early; the numbers cover only the runs that finished.", ""]
    nyra = run.get("nyra")
    info = [f"{summary['tasks']} tasks", f"{run['samples']} sample(s) per task and language",
            f"up to {repairs} repair(s) after the first try", f"{run['timeout_s']:g} s per program"]
    if nyra:
        info.append(f"{nyra['version']}, {run['backend']} backend")
    if run.get("served_models") and not run.get("mock"):
        info.append("served by " + ", ".join(run["served_models"]))
    lines += [", ".join(info) + ".", ""]
    if summary["excluded_runs"]:
        lines += [f"{summary['excluded_runs']} run(s) ended in an API or harness error and are excluded for every "
                  "language (details are in the result file).", ""]
    for w in run.get("warnings", []):
        lines += [f"> Warning: {w}", ""]

    stats = summary["langs"]
    if not summary["runs"]:
        return "\n".join(lines + ["No complete results.", ""])
    names = [DISPLAY.get(lang, lang) for lang in langs]

    # ---- headline
    rows = [["Runs"] + [stats[lang]["n"] for lang in langs],
            ["**pass@1**"] + [_rate_cell(stats[lang]["pass_at_1"], stats[lang]["n"], stats[lang]["pass_at_1_ci"])
                              for lang in langs],
            [f"**pass within {repairs} repairs**"] + [
                _rate_cell(stats[lang]["pass_within_repairs"], stats[lang]["n"], stats[lang]["pass_within_repairs_ci"])
                for lang in langs],
            ["Attempts per run"] + [_num(stats[lang]["avg_attempts"], 2) for lang in langs],
            [f"Output tokens per attempt{est}"] + [_num(stats[lang]["avg_output_tokens_per_attempt"]) for lang in langs],
            [f"Output tokens, first attempt{est}"] + [_num(stats[lang]["avg_output_tokens_first_attempt"]) for lang in langs],
            [f"Output tokens per run{est}"] + [_num(stats[lang]["avg_output_tokens_per_run"]) for lang in langs],
            [f"Code tokens, first attempt{est}"] + [_num(stats[lang]["avg_code_tokens_first_attempt"]) for lang in langs],
            [f"Input tokens per attempt{est}"] + [_num(stats[lang]["avg_input_tokens_per_attempt"]) for lang in langs],
            ["Characters, first attempt"] + [_num(stats[lang]["avg_chars_first_attempt"]) for lang in langs],
            ["Non-blank lines, first attempt"] + [_num(stats[lang]["avg_lines_first_attempt"], 1) for lang in langs]]
    lines += ["## Headline", ""] + _table(["Metric"] + names, rows) + [""]

    # ---- same tasks, both right
    paired = summary.get("paired")
    if paired and len(langs) == 2:
        a, b = langs
        pl = paired["langs"]
        lines += [f"## Same tasks, right on the first try in both languages ({paired['n']} of {summary['runs']} runs)", ""]
        if paired["n"]:
            rows = []
            for label, key in (("Output tokens, first attempt", "avg_output_tokens"), ("Code tokens", "avg_code_tokens"),
                               ("Characters", "avg_chars"), ("Non-blank lines", "avg_lines")):
                digits = 1 if key == "avg_lines" else 0
                rows.append([label, _num(pl[a][key], digits), _num(pl[b][key], digits), _ratio(pl[a][key], pl[b][key])])
            lines += _table(["Metric", names[0], names[1], f"{names[0]} / {names[1]}"], rows) + [""]
            lines += ["These are averages over the runs both languages solved first try, so neither language is "
                      "measured on easy tasks while the other is measured on hard ones.", ""]
        else:
            lines += ["No run was solved first try by both languages.", ""]
        ft = paired.get("first_try")
        if ft:
            lines += [f"First-try agreement (runs): both right {ft['both']}, only {names[0]} {ft['only_' + a]}, "
                      f"only {names[1]} {ft['only_' + b]}, neither {ft['neither']}. Paired sign test over tasks: "
                      f"p = {ft['sign_test_p']:.2f} (a small p means the gap is unlikely to be chance).", ""]

    # ---- how first attempts failed
    kinds = [k for k in FAILURE_KINDS if any(stats[lang]["first_attempt_failures"].get(k) for lang in langs)]
    if kinds:
        rows = [[k] + [stats[lang]["first_attempt_failures"].get(k, 0) for lang in langs] for k in kinds]
        lines += ["## How first attempts failed", ""] + _table(["Failure"] + names, rows) + [""]
    odd = [f"{names[i]}: " + ", ".join(f"{k} x{v}" for k, v in sorted(stats[lang]["odd_stop_reasons"].items()))
           for i, lang in enumerate(langs) if stats[lang]["odd_stop_reasons"]]
    if odd:
        lines += ["Replies that did not end normally (a `max_tokens` stop means the output budget, which thinking "
                  "shares, ran out; `refusal` means the model declined): " + "; ".join(odd) + ".", ""]
    if "nyra" in langs and stats["nyra"]["first_attempt_error_codes"]:
        codes = sorted(stats["nyra"]["first_attempt_error_codes"].items(), key=lambda kv: (-kv[1], kv[0]))
        lines += ["## Compiler errors on Nyra first attempts", "", ", ".join(f"{c} x{n}" for c, n in codes), ""]

    # ---- per task
    by_task: dict = {}
    for r in results["records"]:
        by_task.setdefault(r["task_id"], {}).setdefault(r["lang"], []).append(r)
    rows = []
    for tid in run["task_ids"]:
        t = tasks_by_id.get(tid)
        rows.append([tid, t.min_version if t else "", t.category if t else ""]
                    + [_task_cell(by_task.get(tid, {}).get(lang, []), repairs) for lang in langs])
    lines += ["## Per task", ""] + _table(["Task", "Min Nyra", "Category"] + names, rows) + [""]

    # ---- definitions
    lines += ["## How to read this", "",
              "- **pass@1**: the share of runs whose first program printed exactly the expected output "
              "(95% Wilson interval in brackets, with the number of tasks as the sample size, so repeating tasks does "
              "not narrow it). **pass within N repairs**: the same, when the model may retry up to N times after seeing "
              "the compiler's JSON errors, the Python traceback, or its own wrong output.",
              "- **Output tokens**: what the provider reports as generated, which includes any thinking the model did. "
              "**Code tokens**: the extracted program alone, measured with the provider's token counter. "
              "**Input tokens**: the whole prompt; for Nyra it contains the language spec.",
              "- **Characters / lines**: size of the extracted program (non-blank lines).",
              "- Every language gets the same tasks, the same prompt text, the same repair budget and the same checks. "
              "Methodology and limitations: bench/README.md.",
              f"- With {summary['tasks']} tasks the intervals are wide. --samples averages out run-to-run noise in each "
              "task's result but does not add tasks.", ""]
    ex = run.get("excluded_tasks") or []
    if ex:
        lines += [f"Tasks not run ({len(ex)}): {_group_excluded(ex)}.", ""]
    return "\n".join(lines)
