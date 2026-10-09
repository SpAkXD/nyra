"""Numbers and Markdown for the tiers that go beyond v1: hidden inputs (v2) and edits (edit).

Pure functions on the records `run.py` writes (standard library only).

**Intervals.** The unit of evidence is the task, not the run: five samples of one task are not five tasks. So the
intervals here are a task-level (cluster) bootstrap: resample the *tasks* with replacement, average each task's own
rate over its samples, repeat, and take the middle 95% of the resampled means. `report.py` shows 95% Wilson intervals
with the number of tasks as the sample size; both are given, and they agree when the tasks are many.

**v2** adds what hidden inputs make visible: `example_only` counts first attempts that passed the example in the prompt
and failed a hidden input, which is what a program that prints (or special-cases) the example's answer looks like.

**edit** measures the output tokens per successful edit: all the billed output tokens the model spent on an arm (failed
attempts and failed runs included) divided by the number of edits that worked. An arm that writes less per attempt but
fails more is not cheaper per working edit.
"""

from __future__ import annotations

import random
from typing import Optional

BOOT_ROUNDS = 4000
BOOT_SEED = 20261009


def _mean(values) -> Optional[float]:
    values = [v for v in values if v is not None]
    return sum(values) / len(values) if values else None


def task_bootstrap_ci(per_task: list, rounds: int = BOOT_ROUNDS, seed: int = BOOT_SEED) -> Optional[list]:
    """95% percentile interval of the mean of `per_task` (one rate per task) when the tasks are resampled with
    replacement. None for no tasks; (x, x) when every task has the same rate or there is a single task."""
    n = len(per_task)
    if n == 0:
        return None
    if n == 1 or max(per_task) == min(per_task):
        return [per_task[0], per_task[0]]
    rng = random.Random(seed)
    means = sorted(sum(per_task[rng.randrange(n)] for _ in range(n)) / n for _ in range(rounds))
    return [means[int(0.025 * rounds)], means[min(rounds - 1, int(0.975 * rounds))]]


def per_task_rates(records: list, test) -> dict:
    """{task id: share of that task's runs for which test(record) is true}."""
    by_task: dict = {}
    for r in records:
        by_task.setdefault(r["task_id"], []).append(1.0 if test(r) else 0.0)
    return {t: sum(v) / len(v) for t, v in by_task.items()}


def _valid(r: dict) -> bool:
    return r["status"] in ("pass", "fail")


def _first(r: dict) -> Optional[dict]:
    return r["attempts"][0] if r["attempts"] else None


def _cases(attempt: Optional[dict]) -> list:
    return ((attempt or {}).get("result") or {}).get("cases") or []


def passed_example_failed_hidden(r: dict) -> bool:
    """The first attempt got the example in the prompt right and failed a hidden input."""
    a = _first(r)
    cases = _cases(a)
    return bool(a and not a["result"]["passed"] and cases and cases[0].get("visible") and cases[0].get("passed"))


def passed_example(r: dict) -> bool:
    """The first attempt got the example right (it may or may not have passed the hidden inputs)."""
    a = _first(r)
    if a is None:
        return False
    cases = _cases(a)
    return bool(a["result"]["passed"] or (cases and cases[0].get("visible") and cases[0].get("passed")))


def _rate_block(recs: list, test) -> dict:
    n = len(recs)
    k = sum(1 for r in recs if test(r))
    rates = per_task_rates(recs, test)
    return {"k": k, "n": n, "rate": k / n if n else None, "tasks": len(rates),
            "boot_ci": task_bootstrap_ci(list(rates.values()))}


def v2_summary(records: list, langs: list) -> dict:
    """Per language: pass@1 and pass-within-repairs with task-level bootstrap intervals, the example-only passes, and
    how many tasks were solved on the first try by every sample (all of them) or by none of them."""
    out: dict = {}
    for lang in langs:
        recs = [r for r in records if r["lang"] == lang and _valid(r)]
        first_try = _rate_block(recs, lambda r: r["first_try"])
        within = _rate_block(recs, lambda r: r["status"] == "pass")
        rates = per_task_rates(recs, lambda r: r["first_try"])
        ex = [r for r in recs if passed_example(r)]
        out[lang] = {
            "runs": len(recs), "tasks": first_try["tasks"],
            "pass_at_1": first_try, "pass_within_repairs": within,
            "example_passes": len(ex), "example_only": sum(1 for r in recs if passed_example_failed_hidden(r)),
            "all_samples_pass": sum(1 for v in rates.values() if v == 1.0),
            "no_sample_passes": sum(1 for v in rates.values() if v == 0.0),
        }
    return out


def _sum_known(values) -> Optional[int]:
    values = list(values)
    return sum(values) if values and all(v is not None for v in values) else None


def edit_summary(records: list, arms: list) -> dict:
    """Per arm: how many edits worked and what they cost in output tokens (see the module docstring)."""
    out: dict = {}
    for arm in arms:
        recs = [r for r in records if r["lang"] == arm and _valid(r)]
        passed = [r for r in recs if r["status"] == "pass"]
        attempts = [a for r in recs for a in r["attempts"]]
        total_out = _sum_known(((a.get("usage") or {}).get("output_tokens")) for a in attempts)
        total_cost = _sum_known(((a.get("usage") or {}).get("cost_usd")) for a in attempts)
        first = [r["attempts"][0] for r in recs if r["attempts"]]
        first_ok = [r["attempts"][0] for r in passed if r["first_try"]]
        out[arm] = {
            "runs": len(recs), "tasks": len({r["task_id"] for r in recs}), "passed": len(passed),
            "pass_at_1": _rate_block(recs, lambda r: r["first_try"]),
            "pass_within_repairs": _rate_block(recs, lambda r: r["status"] == "pass"),
            "attempts": len(attempts),
            "output_tokens_total": total_out,
            "output_tokens_per_success": total_out / len(passed) if total_out is not None and passed else None,
            "output_tokens_per_attempt": total_out / len(attempts) if total_out is not None and attempts else None,
            "code_tokens_first_attempt": _mean(a.get("code_tokens") for a in first),
            "output_tokens_first_attempt": _mean((a.get("usage") or {}).get("output_tokens") for a in first),
            "output_tokens_first_attempt_when_first_try_passes":
                _mean((a.get("usage") or {}).get("output_tokens") for a in first_ok),
            "input_tokens_first_attempt": _mean((a.get("usage") or {}).get("input_tokens") for a in first),
            "cost_usd_per_success": total_cost / len(passed) if total_cost is not None and passed else None,
            "rejected_before_running_first_attempt": sum(
                1 for a in first if (a.get("result") or {}).get("kind") == "compile_error"),
        }
    base = arms[0] if arms else None
    for arm in arms:
        a, b = out[arm]["output_tokens_per_success"], out[base]["output_tokens_per_success"] if base else None
        out[arm]["tokens_per_success_vs_baseline"] = a / b if a and b else None
    return out


def tier_summary(tier: str, records: list, langs: list) -> dict:
    """The part of a result's summary that belongs to the tier: {"v2": {...}} or {"edit": {...}}."""
    return {"v2": v2_summary(records, langs)} if tier == "v2" else {"edit": edit_summary(records, langs)}


# ------------------------------------------------------------------------------------------------ rendering


def _pct(x: Optional[float]) -> str:
    return "n/a" if x is None else f"{100 * x:.0f}%"


def _ci(block: dict) -> str:
    if block["n"] == 0:
        return "n/a"
    lo, hi = block["boot_ci"] or [None, None]
    span = f" [{100 * lo:.0f}-{100 * hi:.0f}%]" if lo is not None else ""
    return f"{_pct(block['rate'])} ({block['k']}/{block['n']}){span}"


def _num(x: Optional[float], digits: int = 0) -> str:
    return "n/a" if x is None else f"{x:,.{digits}f}"


def _table(header: list, rows: list) -> list:
    return (["| " + " | ".join(header) + " |", "|" + "|".join("---" for _ in header) + "|"]
            + ["| " + " | ".join(str(c) for c in row) + " |" for row in rows])


def render_tier(results: dict, tier: str) -> str:
    """Markdown for the tier-specific part of a model's summary."""
    import report  # sibling module, imported here so that this one stays importable on its own
    summary = results["summary"]
    langs = results["run"]["langs"]
    names = [report.display(lang) for lang in langs]
    lines: list = []
    if tier == "v2":
        block = summary.get("v2") or {}
        lines += ["## Hidden inputs (v2)", "",
                  "Every program is judged on the example in the prompt and on hidden inputs the model never saw, and "
                  "passes only if it is right on all of them. Intervals are a task-level bootstrap (95%): the unit is the "
                  "task, not the run.", ""]
        rows = [["Tasks / runs"] + [f"{block[l]['tasks']} / {block[l]['runs']}" for l in langs],
                ["**pass@1** (all inputs)"] + [_ci(block[l]["pass_at_1"]) for l in langs],
                ["**pass within repairs**"] + [_ci(block[l]["pass_within_repairs"]) for l in langs],
                ["Right on the example, wrong on a hidden input (first attempt)"]
                + [f"{block[l]['example_only']} of {block[l]['example_passes']}" for l in langs],
                ["Tasks every sample solved first try / no sample did"]
                + [f"{block[l]['all_samples_pass']} / {block[l]['no_sample_passes']}" for l in langs]]
        lines += _table(["Metric"] + names, rows) + [""]
        lines += ["`Right on the example, wrong on a hidden input` is the sign of a program that prints or special-cases "
                  "the example's answer; it is the failure the hidden inputs exist to catch.", ""]
    elif tier == "edit":
        block = summary.get("edit") or {}
        lines += ["## Edits", "",
                  "Each arm changes the same programs for the same change requests. The program that comes out is run "
                  "on the example and on hidden inputs. **Output tokens per successful edit** = all the billed output "
                  "tokens the model spent on the arm, failed attempts included, divided by the edits that worked.", ""]
        base = langs[0]
        rows = [["Edits that worked / runs"] + [f"{block[l]['passed']} / {block[l]['runs']}" for l in langs],
                ["**pass@1**"] + [_ci(block[l]["pass_at_1"]) for l in langs],
                ["**pass within repairs**"] + [_ci(block[l]["pass_within_repairs"]) for l in langs],
                ["**Output tokens per successful edit**"] + [f"**{_num(block[l]['output_tokens_per_success'])}**" for l in langs],
                [f"... relative to {report.display(base)}"]
                + [("n/a" if block[l]["tokens_per_success_vs_baseline"] is None
                    else f"{block[l]['tokens_per_success_vs_baseline']:.2f}x") for l in langs],
                ["Output tokens per attempt"] + [_num(block[l]["output_tokens_per_attempt"]) for l in langs],
                ["Code tokens of the first reply (the edit itself)"] + [_num(block[l]["code_tokens_first_attempt"]) for l in langs],
                ["Input tokens of the first attempt"] + [_num(block[l]["input_tokens_first_attempt"]) for l in langs],
                ["First attempts rejected before running (bad patch or edit)"]
                + [str(block[l]["rejected_before_running_first_attempt"]) for l in langs]]
        if any(block[l]["cost_usd_per_success"] is not None for l in langs):
            rows.append(["Cost per successful edit"] + [("n/a" if block[l]["cost_usd_per_success"] is None
                                                         else f"${block[l]['cost_usd_per_success']:.4f}") for l in langs])
        lines += _table(["Metric"] + names, rows) + [""]
        lines += ["The Nyra arm also reads the language spec in its prompt (see 'Input tokens'); the Python arms do not. "
                  "Only output tokens are compared in the headline row.", ""]
    return "\n".join(lines)
