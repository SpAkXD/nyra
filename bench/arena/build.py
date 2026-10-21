#!/usr/bin/env python3
"""Nyra Arena: one leaderboard (bench/arena/arena.json) from every benchmark result in the repository.

    python bench/arena/build.py                         # -> bench/arena/arena.json
    python bench/arena/build.py --site site/arena       # also write <site>/data.js for nyralang.dev/arena/
    python bench/arena/build.py --check                 # print the ranking, write nothing

Inputs (read only, nothing is copied by hand):
  Websites        bench/web/results/*/runs.csv + summary.json (written by bench/web/report.py), one folder per
                  round. Every folder found is pooled; bench/arena/rounds.json names the rounds and says which Nyra
                  each one used. A folder made by run_model.py carries its own arena.json (model name, provider,
                  prices) next to its results.
  Language tasks  bench/published/*.json (written by bench/publish.py; mock runs and incomplete runs are skipped).
                  Only the Nyra and Python columns are used.
Excluded on purpose: the early OpenRouter runs in bench/results/ (other task set, a tuned prompt, an old card); they
were never published.

Per model and category:
  score       Websites: mean checklist score per answer, a Nyra file that does not build after its one repair turn
              scores 0. Language tasks: share of tasks whose first program passed (pass@1).
  ci          95% percentile bootstrap over runs (10,000 resamples, fixed seed). Websites resample within each
              round x brief cell (the design is balanced: 4 runs per cell); language tasks resample the tasks.
  baseline    the same model on the same briefs in plain HTML (Websites) or on the same tasks in Python (Language).
  rank        by score, ties broken by the lower end of the interval. Only models with >= MIN_RUNS Nyra runs in the
              category rank; the others are listed as provisional. rank_spread is the best and the worst rank the
              intervals allow (1 + models whose interval lies wholly above / 1 + models whose upper end is above
              this model's lower end).
  Overall     the unweighted mean of the two category scores (bootstrap draws averaged pairwise), for models that
              have both.
Standard library only.
"""
from __future__ import annotations

import argparse
import csv
import datetime as dt
import json
import pathlib
import random
import statistics as st
import sys

HERE = pathlib.Path(__file__).resolve().parent
BENCH = HERE.parent
ROOT = BENCH.parent
WEB_RESULTS = BENCH / "web" / "results"
PUBLISHED = BENCH / "published"
sys.path.insert(0, str(BENCH))
try:
    from providers import ANTHROPIC_PRICES  # noqa: E402  (the prices the harness billed with)
except Exception:  # pragma: no cover - the arena still builds without the price table
    ANTHROPIC_PRICES = {}

SCHEMA = "nyra-arena/1"
MIN_RUNS = 8
B = 10_000
SEED = 20261020
REPO = "https://github.com/SpAkXD/nyra"

# Known models: display name, provider, license. A model that run_model.py adds brings its own (results/<dir>/arena.json).
MODELS = {
    "claude-opus-5-5": {"name": "Claude Opus 5.5", "short": "Opus 5.5", "provider": "Anthropic", "license": "Proprietary"},
    "claude-sonnet-5-5": {"name": "Claude Sonnet 5.5", "short": "Sonnet 5.5", "provider": "Anthropic", "license": "Proprietary"},
    "claude-haiku-4-5-20251001": {"name": "Claude Haiku 4.5", "short": "Haiku 4.5", "provider": "Anthropic", "license": "Proprietary"},
}


def model_info(mid: str, extra: dict | None = None) -> dict:
    info = dict(MODELS.get(mid) or {})
    info.update({k: v for k, v in (extra or {}).items() if v is not None})
    info.setdefault("name", mid)
    info.setdefault("short", info["name"])
    info.setdefault("provider", (mid.split("/")[0].title() if "/" in mid else "Unknown"))
    info.setdefault("license", "Unknown")
    if "price" not in info:
        p = ANTHROPIC_PRICES.get(mid)
        info["price"] = {"in": p.input, "out": p.output} if p else None
    return info


# ----------------------------------------------------------------------------------------------- statistics

def boot_cells(cells: list[list[float]], rng: random.Random, b: int = B) -> list[float]:
    """Bootstrap draws of the mean over all values, resampling within each cell (stratified)."""
    n = sum(len(c) for c in cells)
    if not n:
        return []
    draws = []
    for _ in range(b):
        s = 0.0
        for c in cells:
            k = len(c)
            for _ in range(k):
                s += c[int(rng.random() * k)]
        draws.append(s / n)
    return draws


def boot_binomial(k: int, n: int, rng: random.Random, b: int = B) -> list[float]:
    """Bootstrap of a pass rate over n tasks with k passes (resampling tasks is a binomial draw)."""
    p = k / n
    return [sum(1 for _ in range(n) if rng.random() < p) / n for _ in range(b)]


def pct_ci(draws: list[float]) -> list[float] | None:
    if not draws:
        return None
    s = sorted(draws)
    lo = s[int(0.025 * (len(s) - 1))]
    hi = s[int(round(0.975 * (len(s) - 1)))]
    return [round(lo, 4), round(hi, 4)]


def r4(x):
    return None if x is None else round(x, 4)


def mean(xs):
    xs = [x for x in xs if x is not None]
    return sum(xs) / len(xs) if xs else None


# ----------------------------------------------------------------------------------------------- websites

def truthy(v: str):
    return None if v in ("", None) else v == "True"


def num(v: str):
    return None if v in ("", None) else float(v)


def load_rounds_meta() -> dict:
    f = HERE / "rounds.json"
    return json.loads(f.read_text(encoding="utf-8")) if f.exists() else {}


def load_web(ref: str) -> tuple[list, list, dict]:
    """All runs of all web rounds: (rows, rounds, model extras)."""
    meta = load_rounds_meta()
    rows, rounds, extras = [], [], {}
    for d in sorted(p for p in WEB_RESULTS.iterdir() if p.is_dir() and (p / "runs.csv").exists()):
        summary = json.loads((d / "summary.json").read_text(encoding="utf-8")) if (d / "summary.json").exists() else {}
        gen = json.loads((d / "generation.json").read_text(encoding="utf-8")) if (d / "generation.json").exists() else {}
        side = json.loads((d / "arena.json").read_text(encoding="utf-8")) if (d / "arena.json").exists() else {}
        m = {**meta.get(d.name, {}), **side}
        if m.get("exclude"):
            continue
        for mid, extra in (side.get("models") or {}).items():
            extras[mid] = extra
        rid = d.name
        rrows = []
        with open(d / "runs.csv", encoding="utf-8", newline="") as fh:
            for r in csv.DictReader(fh):
                rrows.append({
                    "round": rid, "brief": r["brief"], "model": r["model"], "lang": r["lang"], "run": int(r["run"]),
                    "builds_first": truthy(r["builds_first"]), "builds_after_repair": truthy(r["builds_after_repair"]),
                    "built": truthy(r["built"]), "score": float(r["score"] or 0.0),
                    "code_tokens": num(r["code_tokens"]), "output_tokens": num(r["output_tokens"]),
                    "seconds": num(r["seconds"]), "cost_usd": num(r["cost_usd"]) or 0.0,
                })
        rows += rrows
        date = summary.get("date") or gen.get("finished", "")[:10] or rid[:10]
        rounds.append({
            "id": rid, "category": "web", "date": date, "label": m.get("label", f"Web round {date}"),
            "briefs": summary.get("briefs") or sorted({r["brief"] for r in rrows}),
            "held_out": summary.get("held_out", m.get("held_out")),
            "models": sorted({r["model"] for r in rrows}), "runs": len(rrows),
            "runs_per_cell": summary.get("runs_per_cell"),
            "spent_usd": gen.get("spent_usd"), "nyra": m.get("nyra"), "note": m.get("note"),
            "report": f"{REPO}/blob/{ref}/bench/web/results/{rid}/REPORT.md",
            "folder": f"{REPO}/tree/{ref}/bench/web/results/{rid}",
        })
    return rows, rounds, extras


def web_entry(mid: str, rows: list, rounds: list, rng: random.Random) -> dict:
    ny = [r for r in rows if r["model"] == mid and r["lang"] == "nyra"]
    ht = [r for r in rows if r["model"] == mid and r["lang"] == "html"]

    def cells(rs):
        keys = sorted({(r["round"], r["brief"]) for r in rs})
        return [[r["score"] for r in rs if (r["round"], r["brief"]) == k] for k in keys]

    nd, hd = boot_cells(cells(ny), rng), boot_cells(cells(ht), rng)
    built = [r for r in ny if r["built"]]
    n_cost, h_cost = sum(r["cost_usd"] for r in ny), sum(r["cost_usd"] for r in ht)
    n_work, h_work = sum(r["score"] for r in ny), sum(r["score"] for r in ht)
    ntok, htok = mean([r["code_tokens"] for r in ny]), mean([r["code_tokens"] for r in ht])
    per_round = []
    for rd in rounds:
        a = [r for r in ny if r["round"] == rd["id"]]
        b = [r for r in ht if r["round"] == rd["id"]]
        if a or b:
            per_round.append({"round": rd["id"], "date": rd["date"], "label": rd["label"], "runs": len(a),
                              "score": r4(mean([r["score"] for r in a])), "baseline": r4(mean([r["score"] for r in b])),
                              "builds_first": sum(1 for r in a if r["builds_first"]),
                              "builds_after_repair": sum(1 for r in a if r["builds_after_repair"]),
                              "report": rd["report"]})
    return {
        "model": mid, "_draws": nd, "_bdraws": hd,
        "runs": len(ny), "score": r4(mean([r["score"] for r in ny])), "ci": pct_ci(nd),
        "score_built": r4(mean([r["score"] for r in built])),
        "first": {"k": sum(1 for r in ny if r["builds_first"]), "n": len(ny), "label": "Builds first try"},
        "after": {"k": sum(1 for r in ny if r["builds_after_repair"]), "n": len(ny), "label": "Builds after 1 repair"},
        "baseline": {"lang": "HTML", "runs": len(ht), "score": r4(mean([r["score"] for r in ht])), "ci": pct_ci(hd)},
        "tokens": {"nyra": r4(ntok), "baseline": r4(htok), "ratio": r4(ntok / htok) if ntok and htok else None,
                   "what": "code tokens of the final file, mean over all runs (claude-sonnet-5-5 tokenizer)"},
        "cost": {"answer": r4(n_cost / len(ny)) if ny else None, "working": r4(n_cost / n_work) if n_work else None,
                 "baseline_answer": r4(h_cost / len(ht)) if ht else None, "baseline_working": r4(h_cost / h_work) if h_work else None,
                 "total": r4(n_cost), "baseline_total": r4(h_cost)},
        "seconds": {"nyra": r4(mean([r["seconds"] for r in ny])), "baseline": r4(mean([r["seconds"] for r in ht]))},
        "rounds": per_round,
        "reports": [{"label": p["label"], "url": p["report"]} for p in per_round],
    }


# ----------------------------------------------------------------------------------------------- language tasks

def load_lang(ref: str) -> tuple[list, list]:
    entries_src, rounds = [], []
    for f in sorted(PUBLISHED.glob("*.json")):
        try:
            d = json.loads(f.read_text(encoding="utf-8"))
        except Exception:
            continue
        if not isinstance(d, dict) or d.get("mock") or not d.get("complete") or "models" not in d:
            continue
        run = d.get("run", {})
        langs = run.get("langs") or []
        if "nyra" not in langs or "python" not in langs:
            continue
        name = d.get("name", f.stem)
        date = (run.get("dates") or [d.get("generated", "")])[0]
        rounds.append({
            "id": name, "category": "lang", "date": date,
            "label": f"Language tasks · {(run.get('tasks') or ['?'])[0]} programs",
            "tasks": (run.get("tasks") or [None])[0], "samples": run.get("samples"), "repairs": run.get("repairs"),
            "models": sorted(d["models"]), "runs": sum(m["langs"]["nyra"]["n"] + m["langs"]["python"]["n"] for m in d["models"].values()),
            "spent_usd": r4(sum(m.get("spent_usd") or 0 for m in d["models"].values())),
            "nyra": (run.get("nyra") or {}).get("version"), "held_out": False,
            "report": f"{REPO}/blob/{ref}/bench/published/{name}.md",
            "folder": f"{REPO}/blob/{ref}/bench/published/{name}.json",
        })
        for mid, m in d["models"].items():
            entries_src.append((name, date, mid, m, rounds[-1]))
    return entries_src, rounds


def lang_entry(name, date, mid, m, rd, rng) -> dict:
    ny, py = m["langs"]["nyra"], m["langs"]["python"]
    nd, pd = boot_binomial(ny["pass_at_1"], ny["n"], rng), boot_binomial(py["pass_at_1"], py["n"], rng)
    comp_fail = (ny.get("first_attempt_failures") or {}).get("compile_error", 0)
    pair = ((m.get("paired") or {}).get("pairs") or {}).get("python") or {}
    pn = ((pair.get("langs") or {}).get("nyra") or {}).get("avg_code_tokens")
    pp = ((pair.get("langs") or {}).get("python") or {}).get("avg_code_tokens")
    return {
        "model": mid, "_draws": nd, "_bdraws": pd,
        "runs": ny["n"], "score": r4(ny["pass_at_1_rate"]), "ci": pct_ci(nd),
        "first": {"k": ny["n"] - comp_fail, "n": ny["n"], "label": "Compiles first try"},
        "after": {"k": ny["pass_within_repairs"], "n": ny["n"], "label": f"Passes within {rd.get('repairs')} repairs"},
        "baseline": {"lang": "Python", "runs": py["n"], "score": r4(py["pass_at_1_rate"]), "ci": pct_ci(pd),
                     "after": py["pass_within_repairs"]},
        "tokens": {"nyra": r4(pn), "baseline": r4(pp), "ratio": r4(pn / pp) if pn and pp else None,
                   "what": f"code tokens, first attempt, on the {pair.get('n')} tasks both languages passed first try"},
        "cost": {"answer": r4(ny["avg_cost_per_run_usd"]),
                 "working": r4(ny["total_cost_usd"] / ny["pass_within_repairs"]) if ny["pass_within_repairs"] else None,
                 "baseline_answer": r4(py["avg_cost_per_run_usd"]),
                 "baseline_working": r4(py["total_cost_usd"] / py["pass_within_repairs"]) if py["pass_within_repairs"] else None,
                 "total": r4(ny["total_cost_usd"]), "baseline_total": r4(py["total_cost_usd"])},
        "seconds": None,
        "rounds": [{"round": name, "date": date, "label": rd["label"], "runs": ny["n"], "score": r4(ny["pass_at_1_rate"]),
                    "baseline": r4(py["pass_at_1_rate"]), "report": rd["report"]}],
        "reports": [{"label": f"{name} (published summary)", "url": rd["report"]}],
    }


# ----------------------------------------------------------------------------------------------- ranking

def rank(entries: list) -> list:
    ranked = [e for e in entries if not e["provisional"] and e["score"] is not None]
    ranked.sort(key=lambda e: (-e["score"], -(e["ci"] or [0, 0])[0], e["name"]))
    for i, e in enumerate(ranked):
        lo, hi = e["ci"] or [e["score"], e["score"]]
        others = [o for o in ranked if o is not e]
        e["rank"] = i + 1
        e["rank_spread"] = [1 + sum(1 for o in others if (o["ci"] or [o["score"]] * 2)[0] > hi),
                            1 + sum(1 for o in others if (o["ci"] or [o["score"]] * 2)[1] > lo)]
    prov = sorted((e for e in entries if e not in ranked), key=lambda e: (-(e["score"] or 0), e["name"]))
    for e in prov:
        e["rank"] = None
        e["rank_spread"] = None
    return ranked + prov


def decorate(e: dict, info: dict) -> dict:
    e.update({"name": info["name"], "short": info["short"], "provider": info["provider"], "license": info["license"],
              "price": info.get("price")})
    e["provisional"] = e["runs"] < MIN_RUNS
    return e


def build(ref: str = "v0.8") -> dict:
    rng = random.Random(SEED)
    web_rows, web_rounds, extras = load_web(ref)
    lang_src, lang_rounds = load_lang(ref)

    web = []
    for mid in sorted({r["model"] for r in web_rows}):
        e = web_entry(mid, web_rows, web_rounds, rng)
        if e["runs"]:
            web.append(decorate(e, model_info(mid, extras.get(mid))))
    lang = []
    latest = {}
    for name, date, mid, m, rd in lang_src:  # newest published run per model
        if mid not in latest or date >= latest[mid][1]:
            latest[mid] = (name, date, mid, m, rd)
    for mid in sorted(latest):
        lang.append(decorate(lang_entry(*latest[mid], rng), model_info(mid, extras.get(mid))))

    overall = []
    by_w, by_l = {e["model"]: e for e in web}, {e["model"]: e for e in lang}
    for mid in sorted(set(by_w) & set(by_l)):
        w, l = by_w[mid], by_l[mid]
        d = [(a + b) / 2 for a, b in zip(w["_draws"], l["_draws"])]
        bd = [(a + b) / 2 for a, b in zip(w["_bdraws"], l["_bdraws"])]
        e = {"model": mid, "_draws": d, "_bdraws": bd, "runs": w["runs"] + l["runs"],
             "score": r4((w["score"] + l["score"]) / 2), "ci": pct_ci(d),
             "baseline": {"lang": "HTML / Python", "runs": w["baseline"]["runs"] + l["baseline"]["runs"],
                          "score": r4((w["baseline"]["score"] + l["baseline"]["score"]) / 2), "ci": pct_ci(bd)},
             "parts": {"web": {"score": w["score"], "ci": w["ci"], "baseline": w["baseline"]["score"], "runs": w["runs"]},
                       "lang": {"score": l["score"], "ci": l["ci"], "baseline": l["baseline"]["score"], "runs": l["runs"]}},
             "first": None, "after": None, "tokens": None, "cost": None, "seconds": None,
             "rounds": w["rounds"] + l["rounds"], "reports": w["reports"] + l["reports"]}
        decorate(e, model_info(mid, extras.get(mid)))
        e["provisional"] = w["provisional"] or l["provisional"]
        overall.append(e)

    cats = [
        {"id": "overall", "name": "Overall", "baseline": "HTML / Python", "entries": rank(overall),
         "score_def": "Unweighted mean of the Websites score and the Language tasks score.",
         "unit": "mean of the two category scores"},
        {"id": "web", "name": "Websites", "baseline": "HTML", "entries": rank(web),
         "score_def": "Mean functional-checklist score per site on held-out client briefs. A Nyra file that does not build "
                      "after one repair turn scores 0.",
         "unit": "checklist score, failed builds = 0"},
        {"id": "lang", "name": "Language tasks", "baseline": "Python", "entries": rank(lang),
         "score_def": "Share of the 83 small programs whose first answer printed the exact expected output (pass@1).",
         "unit": "first-try pass rate"},
    ]
    for c in cats:
        for e in c["entries"]:
            e.pop("_draws", None)
            e.pop("_bdraws", None)
    rounds = sorted(web_rounds + lang_rounds, key=lambda r: (r["date"], r["id"]), reverse=True)
    all_models = {e["model"] for c in cats for e in c["entries"]}
    total_runs = sum(r["runs"] for r in rounds)
    return {
        "schema": SCHEMA,
        "updated": max(r["date"] for r in rounds) if rounds else None,
        "built": dt.date.today().isoformat(),
        "min_runs": MIN_RUNS,
        "bootstrap": {"resamples": B, "seed": SEED, "interval": "95% percentile",
                      "web": "resample runs within each round x brief cell", "lang": "resample tasks"},
        "totals": {"graded_answers": total_runs, "models": len(all_models), "rounds": len(rounds),
                   "spent_usd": r4(sum(r.get("spent_usd") or 0 for r in rounds))},
        "categories": cats,
        "rounds": rounds,
        "excluded": ["Early OpenRouter runs (Gemini, DeepSeek, Haiku via OpenRouter, 2026-10-07/08) and the bakery web "
                     "pilot: another task set, a tuned prompt and an old card; never published and not comparable."],
        "sources": {"web": f"{REPO}/tree/{ref}/bench/web/results", "lang": f"{REPO}/tree/{ref}/bench/published",
                    "arena": f"{REPO}/tree/{ref}/bench/arena"},
    }


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--out", default=str(HERE / "arena.json"))
    ap.add_argument("--site", default="", help="a site folder (e.g. site/arena): also write data.js there")
    ap.add_argument("--ref", default="v0.8", help="git ref the report links point at (default v0.8)")
    ap.add_argument("--check", action="store_true", help="print the ranking only")
    a = ap.parse_args(argv)
    data = build(a.ref)
    for c in data["categories"]:
        print(f"== {c['name']} ({c['unit']})")
        for e in c["entries"]:
            rk = e["rank"] if e["rank"] else "prov."
            sp = f"{e['rank_spread'][0]}-{e['rank_spread'][1]}" if e["rank_spread"] else "-"
            ci = e["ci"] or [None, None]
            print(f"  {rk:>5} [{sp:>3}] {e['name']:<20} {e['score']:.3f} [{ci[0]:.3f}, {ci[1]:.3f}]  runs {e['runs']:>3}  "
                  f"{e['baseline']['lang']} {e['baseline']['score']:.3f}")
    if a.check:
        return 0
    text = json.dumps(data, indent=1, ensure_ascii=False)
    pathlib.Path(a.out).write_text(text + "\n", encoding="utf-8", newline="\n")
    print("wrote", a.out)
    if a.site:
        site = pathlib.Path(a.site)
        site.mkdir(parents=True, exist_ok=True)
        js = ("/* Nyra Arena data for nyralang.dev/arena/. Generated by bench/arena/build.py from bench/arena/arena.json;\n"
              "   do not edit by hand. Sources: the runs.csv of every folder in bench/web/results (Websites) and the\n"
              "   summaries in bench/published (Language tasks). */\nwindow.NYRA_ARENA = " + json.dumps(data, ensure_ascii=False, separators=(",", ":")) + ";\n")
        (site / "data.js").write_text(js, encoding="utf-8", newline="\n")
        print("wrote", site / "data.js")
    return 0


if __name__ == "__main__":
    sys.exit(main())
