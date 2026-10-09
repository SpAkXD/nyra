"""Waterfall of the mechanical rewrites on the three corpora (Claude tokenizer), and the hand-written 24-program sample.

Output: packages.json.  The waterfall applies the steps cumulatively in the order below, so each row is the *marginal* saving
and the rows add up (unlike ablate.py, where every rewrite is applied alone)."""
import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import glob, json
import data
import rewrites as R
import rewrites2 as R2
from tokcount import count_many

M = dict(R.SYNTAX); M.update(R2.IDIOM)
here = os.path.dirname(os.path.abspath(__file__))
CORP = {"ref": data.reference(), "opus": data.model_pairs("opus"), "sonnet": data.model_pairs("sonnet")}
LIB = ["neg_index", "in_op", "ternary", "pad_spec", "bare_lambda", "comprehension"]  # interp is token-neutral; pad_spec needs it
STEPS = [("unwrap_main", ["unwrap_main"]),
         ("library idioms (in, [-1], a if c else b, {x:>3}, .map(int), comprehension)", ["interp"] + LIB),
         ("implicit return", ["implicit_ret"]),
         ("infer empty-collection types", ["drop_local_ann"]),
         ("infer return types", ["drop_ret_types"]),
         ("indentation blocks", ["indent_blocks"]),
         ("| 0.6-X mechanical stops here", []),
         ("no let/var keyword", ["drop_let_var"]),
         ("infer parameter types", ["drop_param_types"]),
         ("positional struct constructors", ["positional_struct"]),
         ("| 0.6-Y mechanical stops here", [])]

texts, index = [], {}


def want(t):
    if t not in index:
        index[t] = len(texts); texts.append(t)
    return index[t]


plan = []
for cn, c in CORP.items():
    for task, langs in c.items():
        ny = data.strip_nyra_comments(langs["nyra"])
        py = data.strip_py_comments(langs["python"])
        row = {"cn": cn, "task": task, "py": want(py), "stages": [want(ny)]}
        s = ny
        for name, fs in STEPS:
            for f in fs:
                s = M[f](s)
            row["stages"].append(want(s))
            if name.startswith("|"):
                row.setdefault("kw", []).append(want(R.rename_keywords(s)))
        plan.append(row)

# sample (hand written)
sample = []
for p in sorted(glob.glob(f"{here}/sample/X/*.nyra")):
    t = os.path.basename(p)[:-5]
    rd = lambda q: open(q, encoding="utf-8").read().strip("\n")
    x, y = rd(p), rd(f"{here}/sample/Y/{t}.nyra")
    sample.append({"task": t, "py": want(rd(f"{here}/sample/py/{t}.py")), "orig": want(rd(f"{here}/sample/orig/{t}.nyra")),
                   "X": want(x), "Xkw": want(R.rename_keywords(x)), "Y": want(y), "Ykw": want(R.rename_keywords(y))})

c = count_many(texts)
res = {"corpora": {}, "sample": {}}
for cn in CORP:
    sub = [r for r in plan if r["cn"] == cn]
    py = sum(c[r["py"]] for r in sub)
    stages = [sum(c[r["stages"][i]] for r in sub) for i in range(len(STEPS) + 1)]
    res["corpora"][cn] = {"n": len(sub), "python": py, "stages": stages,
                          "mean_of_ratios": [sum(c[r["stages"][i]] / c[r["py"]] for r in sub) / len(sub) for i in range(len(STEPS) + 1)]}
    print(f"== {cn} n={len(sub)} python {py}")
    res["corpora"][cn]["kw_stages"] = [sum(c[r["kw"][i]] for r in sub) for i in range(2)]
    print(f"   X+keywords {res['corpora'][cn]['kw_stages'][0]}  ratio {res['corpora'][cn]['kw_stages'][0]/py:.3f};  Y+keywords {res['corpora'][cn]['kw_stages'][1]}  ratio {res['corpora'][cn]['kw_stages'][1]/py:.3f}")
    print(f"   {'orig':74} {stages[0]:7}  ratio {stages[0]/py:.3f}")
    for i, (name, fs) in enumerate(STEPS):
        print(f"   {name:74} {stages[i+1]:7}  ratio {stages[i+1]/py:.3f}  step {stages[i]-stages[i+1]:+5}")
tot = {k: sum(c[r[k]] for r in sample) for k in ("py", "orig", "X", "Xkw", "Y", "Ykw")}
res["sample"] = tot
print("sample (24 hand-written programs):", tot, {k: round(v / tot["py"], 3) for k, v in tot.items()})
# mechanical counterparts on the same 24 tasks
tasks = {s["task"] for s in sample}
mech = {}
for cn in ("opus",):
    sub = [r for r in plan if r["cn"] == cn and r["task"] in tasks]
    mech = {"mechX": sum(c[r["stages"][7]] for r in sub), "mechY": sum(c[r["stages"][11]] for r in sub), "mechXkw": sum(c[r["kw"][0]] for r in sub), "mechYkw": sum(c[r["kw"][1]] for r in sub),
            "orig": sum(c[r["stages"][0]] for r in sub), "py": sum(c[r["py"]] for r in sub)}
res["sample_mech"] = mech
print("sample mechanical:", mech)
json.dump(res, open(f"{here}/packages" + ("_" + os.environ["TOK_MODEL"] if os.environ.get("TOK_MODEL") else "") + ".json", "w"), indent=1)
