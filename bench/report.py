"""Metrics and the Markdown summaries of benchmark runs (pure functions, standard library only).

Metrics are computed on the runs for which *every* language produced a valid result (status
pass or fail). A run that ended in an API error is dropped for all languages, so the languages
are always compared on exactly the same tasks.

The token headline is **code tokens** (the extracted program alone, counted with the model's own
tokenizer) with the **billed output tokens** (what the API charged for the reply, thinking included)
next to it. Code tokens say how compact the language is; billed tokens say what it cost.
"""

from __future__ import annotations

import math
from collections import Counter
from typing import Optional

FAILURE_KINDS = ("no_code", "compile_error", "toolchain_error", "runtime_error", "timeout", "output_limit",
                 "wrong_output")
DISPLAY = {"nyra": "Nyra", "python": "Python", "typescript": "TypeScript", "rust": "Rust"}
LANG_ORDER = ("nyra", "python", "typescript", "rust")  # the languages, in table-column order (run.py uses it too)


def display(lang: str) -> str:
    return DISPLAY.get(lang, lang)


def ordered_langs(langs) -> list:
    """Known languages in the standard column order, then any others alphabetically."""
    langs = set(langs)
    return [lang for lang in LANG_ORDER if lang in langs] + sorted(langs - set(LANG_ORDER))


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


def _reply_tokens(attempt: dict) -> Optional[int]:
    """Tokens of the visible reply: billed output minus thinking, when the API reports the thinking."""
    out, thinking = _usage(attempt, "output_tokens"), _usage(attempt, "reasoning_tokens")
    return out - thinking if out is not None and thinking is not None else None


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
    costs = [_usage(a, "cost_usd") for a in every]
    total_cost = sum(c for c in costs if c is not None) if any(c is not None for c in costs) else None
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
        "avg_reasoning_tokens_first_attempt": mean(_usage(a, "reasoning_tokens") for a in first),
        "avg_reply_tokens_first_attempt": mean(_reply_tokens(a) for a in first),
        "avg_input_tokens_per_attempt": mean(_usage(a, "input_tokens") for a in every),
        "avg_input_tokens_first_attempt": mean(_usage(a, "input_tokens") for a in first),
        "avg_code_tokens_first_attempt": mean(a.get("code_tokens") for a in first),
        "avg_chars_first_attempt": mean(a.get("chars") for a in first),
        "avg_lines_first_attempt": mean(a.get("lines") for a in first),
        "total_input_tokens": sum(_usage(a, "input_tokens") or 0 for a in every),
        "total_output_tokens": sum(_usage(a, "output_tokens") or 0 for a in every),
        "total_cost_usd": total_cost,
        "avg_cost_per_run_usd": total_cost / n if total_cost is not None and n else None,
        "attempts_total": len(every),
        "odd_stop_reasons": dict(Counter(a["stop_reason"] for a in every if a.get("stop_reason") not in (None, "end_turn"))),
        "first_attempt_failures": dict(fail_kinds),
        "first_attempt_error_codes": dict(codes),
        "toolchain_errors": sum(1 for a in every if a["result"]["kind"] == "toolchain_error"),
    }


def _first_attempts_metrics(attempts: list) -> dict:
    return {
        "avg_output_tokens": mean(_usage(a, "output_tokens") for a in attempts),
        "avg_code_tokens": mean(a.get("code_tokens") for a in attempts),
        "avg_chars": mean(a.get("chars") for a in attempts),
        "avg_lines": mean(a.get("lines") for a in attempts),
    }


def _pair_stats(by_key: dict, keys: list, a: str, b: str) -> dict:
    """Language `a` (the baseline) against `b`: token and size averages on the runs both got right on the first
    try, and how the first-try results agree."""
    both = [k for k in keys if by_key[k][a]["first_try"] and by_key[k][b]["first_try"]]
    only_a = sum(1 for k in keys if by_key[k][a]["first_try"] and not by_key[k][b]["first_try"])
    only_b = sum(1 for k in keys if by_key[k][b]["first_try"] and not by_key[k][a]["first_try"])
    neither = sum(1 for k in keys if not by_key[k][a]["first_try"] and not by_key[k][b]["first_try"])
    per_task: dict = {}  # task -> [first-try passes of a, of b, runs]
    for k in keys:
        row = per_task.setdefault(k[0], [0, 0, 0])
        row[0] += by_key[k][a]["first_try"]
        row[1] += by_key[k][b]["first_try"]
        row[2] += 1
    diffs = [(ra - rb) / runs for ra, rb, runs in per_task.values()]
    return {
        "a": a, "b": b, "n": len(both),
        "langs": {a: _first_attempts_metrics([by_key[k][a]["attempts"][0] for k in both]),
                  b: _first_attempts_metrics([by_key[k][b]["attempts"][0] for k in both])},
        "first_try": {"both": len(both), "only_a": only_a, "only_b": only_b, "neither": neither,
                      "sign_test_p": paired_sign_test(diffs)},
    }


def _by_category(by_key: dict, keys: list, langs: list, categories: dict) -> dict:
    out: dict = {}
    for k in keys:
        cat = categories.get(k[0])
        if cat is None:
            continue
        for lang in langs:
            rec = by_key[k][lang]
            cell = out.setdefault(cat, {}).setdefault(lang, {"n": 0, "pass_at_1": 0, "pass_within_repairs": 0})
            cell["n"] += 1
            cell["pass_at_1"] += 1 if rec["first_try"] else 0
            cell["pass_within_repairs"] += 1 if rec["status"] == "pass" else 0
    return dict(sorted(out.items()))


def summarize(records: list, langs: list, categories: Optional[dict] = None) -> dict:
    """Statistics of one model's run. `categories` maps task id -> category (for the per-category breakdown).

    "paired": the first language is the baseline. `n` and `langs`: the runs every language got right on the first
    try, with each language's averages on exactly those runs. `pairs`: for each other language, the runs the baseline
    and that language both got right on the first try (more runs than the all-language set), the averages on them
    and the first-try agreement with its paired sign test."""
    by_key: dict = {}
    for r in records:
        by_key.setdefault((r["task_id"], r["sample"]), {})[r["lang"]] = r
    keys = sorted(k for k, d in by_key.items() if all(_valid(d.get(lang)) for lang in langs))
    out: dict = {
        "tasks": len({k[0] for k in keys}), "runs": len(keys), "excluded_runs": len(by_key) - len(keys),
        "langs": {lang: _lang_stats([by_key[k][lang] for k in keys]) for lang in langs}, "paired": None,
        "by_category": None,
    }
    if keys:
        both_first = [k for k in keys if all(by_key[k][lang]["first_try"] for lang in langs)]
        paired: dict = {"baseline": langs[0], "n": len(both_first), "langs": {}, "pairs": {}}
        for lang in langs:
            paired["langs"][lang] = _first_attempts_metrics([by_key[k][lang]["attempts"][0] for k in both_first])
        for other in langs[1:]:
            paired["pairs"][other] = _pair_stats(by_key, keys, langs[0], other)
        out["paired"] = paired
        if categories:
            out["by_category"] = _by_category(by_key, keys, langs, categories)
    return out


# ------------------------------------------------------------------------------ rendering


def _pct(rate: Optional[float]) -> str:
    return "n/a" if rate is None else f"{100 * rate:.0f}%"


def _rate_cell(k: int, n: int, ci) -> str:
    if n == 0:
        return "n/a"
    return f"{_pct(k / n)} ({k}/{n}) [{100 * ci[0]:.0f}-{100 * ci[1]:.0f}%]"


def _short_rate(k: int, n: int) -> str:
    return "n/a" if n == 0 else f"{_pct(k / n)} ({k}/{n})"


def _num(value: Optional[float], digits: int = 0) -> str:
    return "n/a" if value is None else f"{value:,.{digits}f}"


def _usd(value: Optional[float]) -> str:
    if value is None:
        return "n/a"
    return f"${value:,.4f}" if abs(value) < 1 else f"${value:,.2f}"


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


def model_label(results: dict) -> str:
    return results["run"]["provider"]["model"]


def _est(results: dict) -> str:
    return " (estimated)" if results["run"].get("tokens_are_estimates") else ""


def _tokens_pair(stats: dict) -> str:
    """`code tokens (billed output tokens)` of the first attempt, the token headline in one table cell."""
    code, billed = stats["avg_code_tokens_first_attempt"], stats["avg_output_tokens_first_attempt"]
    return f"{_num(code)} ({_num(billed)})"


def headline_rows(stats: dict, langs: list, repairs: int, full: bool = True) -> list:
    """The rows of a model's headline table, one column per language. `full=False` keeps the ones worth publishing."""
    def row(label, fn):
        return [label] + [fn(stats[lang]) for lang in langs]

    rows = [row("Runs", lambda s: s["n"]),
            row("**pass@1**", lambda s: _rate_cell(s["pass_at_1"], s["n"], s["pass_at_1_ci"])),
            row(f"**pass within {repairs} repairs**",
                lambda s: _rate_cell(s["pass_within_repairs"], s["n"], s["pass_within_repairs_ci"])),
            row("Attempts per run", lambda s: _num(s["avg_attempts"], 2)),
            row("**Code tokens, first attempt**", lambda s: _num(s["avg_code_tokens_first_attempt"])),
            row("**Billed output tokens, first attempt** (incl. thinking)",
                lambda s: _num(s["avg_output_tokens_first_attempt"]))]
    if any(stats[lang]["avg_reasoning_tokens_first_attempt"] is not None for lang in langs):
        rows.append(row("of which thinking, first attempt", lambda s: _num(s["avg_reasoning_tokens_first_attempt"])))
        if full:
            rows.append(row("Reply tokens without thinking, first attempt",
                            lambda s: _num(s["avg_reply_tokens_first_attempt"])))
    rows.append(row("Billed output tokens per run", lambda s: _num(s["avg_output_tokens_per_run"])))
    if full:
        rows.insert(-1, row("Billed output tokens per attempt", lambda s: _num(s["avg_output_tokens_per_attempt"])))
        rows.append(row("Input tokens per attempt", lambda s: _num(s["avg_input_tokens_per_attempt"])))
    rows += [row("Characters, first attempt", lambda s: _num(s["avg_chars_first_attempt"])),
             row("Non-blank lines, first attempt", lambda s: _num(s["avg_lines_first_attempt"], 1))]
    if any(stats[lang]["total_cost_usd"] is not None for lang in langs):
        rows.append(row("Cost per run (as billed)", lambda s: _usd(s["avg_cost_per_run_usd"])))
    return rows


def render_markdown(results: dict, tasks_by_id: dict) -> str:
    """The summary of one model's run (the .md next to its .json)."""
    run, summary = results["run"], results["summary"]
    langs = run["langs"]
    repairs = run["repairs"]
    est = _est(results)
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
    if run.get("node"):
        info.append(f"Node.js {run['node']['version'].lstrip('v')}")
    if run.get("rust"):
        info.append(run["rust"]["version"])
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
    names = [display(lang) for lang in langs]

    # ---- headline
    lines += ["## Headline", ""] + _table(["Metric"] + names, headline_rows(stats, langs, repairs)) + [""]
    if est:
        lines += ["Token counts are character-based estimates (mock provider).", ""]
    if all(stats[lang]["avg_code_tokens_first_attempt"] is None for lang in langs):
        lines += ["Code tokens could not be measured in this run (the provider has no token counter, it was switched "
                  "off, or counting failed); the billed output tokens are what the API charged.", ""]

    # ---- same tasks, right the first time
    paired = summary.get("paired")
    if paired and len(langs) > 1:
        lines += _render_paired(paired, langs, names, summary["runs"])

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

    # ---- per category
    if summary.get("by_category"):
        lines += _render_categories(summary["by_category"], langs, names, repairs)

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
              "the compiler's JSON errors, the interpreter's or compiler's messages, or its own wrong output.",
              "- **Code tokens**: the extracted program alone, counted with the model's own tokenizer: how compact the "
              "language is. **Billed output tokens**: what the API charged for the reply, thinking included: what it cost. "
              "**Input tokens**: the whole prompt; for Nyra it contains the language spec.",
              "- **Characters / lines**: size of the extracted program (non-blank lines).",
              "- Every language gets the same tasks, the same task prompt, the same repair budget and the same checks. "
              "Methodology and limitations: bench/README.md.",
              f"- With {summary['tasks']} tasks the intervals are wide. --samples averages out run-to-run noise in each "
              "task's result but does not add tasks.", ""]
    ex = run.get("excluded_tasks") or []
    if ex:
        lines += [f"Tasks not run ({len(ex)}): {_group_excluded(ex)}.", ""]
    return "\n".join(lines)


def _render_paired(paired: dict, langs: list, names: list, runs: int, h: str = "##") -> list:
    lines = []
    base = langs[0]
    pairs = paired.get("pairs") or {}
    if pairs:
        rows = []
        for other, p in pairs.items():
            a, b = p["langs"][base], p["langs"][other]
            ft = p["first_try"]
            rows.append([display(other), p["n"],
                         f"{_num(a['avg_code_tokens'])} / {_num(b['avg_code_tokens'])}",
                         _ratio(a["avg_code_tokens"], b["avg_code_tokens"]),
                         f"{_num(a['avg_output_tokens'])} / {_num(b['avg_output_tokens'])}",
                         _ratio(a["avg_output_tokens"], b["avg_output_tokens"]),
                         _ratio(a["avg_chars"], b["avg_chars"]),
                         f"{ft['only_a']} / {ft['only_b']}", f"{ft['sign_test_p']:.2f}"])
        bn = display(base)
        lines += [f"{h} {bn} against each other language, on the runs both got right on the first try", ""]
        lines += _table(["Compared with", "Runs both right", f"Code tokens ({bn} / other)", "Ratio",
                         f"Billed output tokens ({bn} / other)", "Ratio", "Characters ratio",
                         f"First try only {bn} / only other", "Sign test p"], rows) + [""]
        lines += [f"Each row averages over the runs in which {bn} and that language were both right on the first try, so "
                  "neither is measured on easy tasks while the other is measured on hard ones. A ratio below 1.00x means "
                  f"{bn} used fewer. The sign test compares the first-try results task by task (a small p means the "
                  "gap is unlikely to be chance).", ""]
    if paired["n"] and len(langs) > 2:
        pl = paired["langs"]
        rows = []
        for label, key in (("Code tokens", "avg_code_tokens"), ("Billed output tokens, first attempt", "avg_output_tokens"),
                           ("Characters", "avg_chars"), ("Non-blank lines", "avg_lines")):
            digits = 1 if key == "avg_lines" else 0
            rows.append([label] + [_num(pl[lang][key], digits) for lang in langs])
        lines += [f"{h} Same runs, right on the first try in all {len(langs)} languages ({paired['n']} of {runs} runs)", ""]
        lines += _table(["Metric"] + names, rows) + [""]
    elif paired["n"] == 0 and len(langs) > 2:
        lines += [f"No run was solved first try in all {len(langs)} languages.", ""]
    return lines


def _category_cell(cell: Optional[dict]) -> str:
    if not cell:
        return "-"
    return f"{cell['pass_at_1']}/{cell['n']} ({cell['pass_within_repairs']}/{cell['n']})"


def _render_categories(by_category: dict, langs: list, names: list, repairs: int, h: str = "##") -> list:
    rows = [[cat] + [_category_cell(cells.get(lang)) for lang in langs] for cat, cells in by_category.items()]
    return ([f"{h} Per category", "", f"First try (within {repairs} repairs), as passed runs / runs.", ""]
            + _table(["Category"] + names, rows) + [""])


# ---------------------------------------------------------------------- model x language


def _grid(results_list: list, langs: list, cell, title: str, note: Optional[str] = None, h: str = "##") -> list:
    """One table: a row per model, a column per language."""
    rows = []
    for results in results_list:
        stats = results["summary"]["langs"]
        rows.append([model_label(results)] + [cell(results, stats[lang]) if lang in stats else "-" for lang in langs])
    out = [f"{h} {title}", ""]
    if note:
        out += [note, ""]
    return out + _table(["Model"] + [display(lang) for lang in langs], rows) + [""]


def comparison_langs(results_list: list) -> list:
    return ordered_langs({lang for results in results_list for lang in results["run"]["langs"]})


def render_comparison(results_list: list) -> str:
    """Model x language tables over several runs (one results dict per model, as written by run.py)."""
    if not results_list:
        return "No results.\n"
    langs = comparison_langs(results_list)
    first = results_list[0]["run"]
    repairs = first["repairs"]
    mock = any(r["run"].get("mock") for r in results_list)
    est = " (estimated)" if any(r["run"].get("tokens_are_estimates") for r in results_list) else ""
    lines = [f"# Nyra benchmark: model comparison, {first['date']}", ""]
    if mock:
        lines += ["> **MOCK RUN: this is a self-test of the pipeline, not a measurement.** The \"models\" replay the "
                  "reference solutions (`mock-*` models break some first attempts on purpose), and token counts are "
                  "character-based estimates.", ""]
    if any(not r["run"].get("complete", True) for r in results_list):
        lines += ["> **INCOMPLETE:** at least one model was stopped early; its numbers cover only the runs that "
                  "finished.", ""]
    tasks = sorted({r["summary"]["tasks"] for r in results_list})
    lines += [f"{len(results_list)} model(s) x {len(langs)} language(s); {', '.join(str(t) for t in tasks)} tasks, "
              f"{first['samples']} sample(s) per task and language, up to {repairs} repair(s) after the first try. "
              "Every cell is computed on the runs where all of that model's languages produced a valid result.", ""]
    if len({(r["run"].get("tasks_sha256"), r["run"].get("samples"), r["run"].get("repairs")) for r in results_list}) > 1:
        lines += ["> Warning: these runs differ in task set, samples or repairs; compare with care.", ""]

    return "\n".join(lines + comparison_tables(results_list))


def comparison_tables(results_list: list, h: str = "##") -> list:
    """The model x language tables (first-try success, success within repairs, tokens, cost) and a table of
    the models themselves, as Markdown lines. Shared by the run's comparison file and by publish.py."""
    langs = comparison_langs(results_list)
    repairs = results_list[0]["run"]["repairs"]
    est = " (estimated)" if any(r["run"].get("tokens_are_estimates") for r in results_list) else ""
    lines = _grid(results_list, langs, lambda r, s: _short_rate(s["pass_at_1"], s["n"]),
                  "First-try success (pass@1)", "Passed runs / runs.", h)
    lines += _grid(results_list, langs, lambda r, s: _short_rate(s["pass_within_repairs"], s["n"]),
                   f"Success within {repairs} repairs", None, h)
    lines += _grid(results_list, langs, lambda r, s: _tokens_pair(s),
                   f"Tokens of the first attempt: code tokens (billed output tokens){est}",
                   "Code tokens count the program alone; billed output tokens are what the API charged for the reply, "
                   "thinking included.", h)
    if any(s["total_cost_usd"] is not None for r in results_list for s in r["summary"]["langs"].values()):
        lines += _grid(results_list, langs, lambda r, s: _usd(s["avg_cost_per_run_usd"]),
                       "Cost per run (as billed)", None, h)
    rows = []
    for results in results_list:
        run = results["run"]
        spent = run.get("spent_usd")
        rows.append([model_label(results), ", ".join(run.get("served_models") or []) or "-",
                     "yes" if run.get("complete", True) else "NO", _usd(spent) if spent is not None else "-"])
    lines += [f"{h} Models", ""] + _table(["Model", "Served as", "Complete", "Total spent (all requests)"], rows) + [""]
    return lines
