"""Idiom rewrites alone and the cumulative packages (mechanical part).  Output: ablate2.json"""
import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import json
import data
import rewrites as R
import rewrites2 as R2
from tokcount import count_many

M = dict(R.SYNTAX)
M.update(R2.IDIOM)
CORP = {"ref": data.reference(), "opus": data.model_pairs("opus"), "sonnet": data.model_pairs("sonnet")}

IDIOMS = ["neg_index", "in_op", "ternary", "interp", "pad_spec", "bare_lambda", "comprehension"]
L1 = ["neg_index", "in_op", "ternary", "interp", "pad_spec", "bare_lambda", "comprehension"]
PKG = {
    "A_docs_script": ["unwrap_main"],
    "B_lib": ["unwrap_main"] + L1,
    "C_lib+indent": ["unwrap_main"] + L1 + ["indent_blocks"],
    "D_lib+indent+implicit_ret": ["unwrap_main"] + L1 + ["implicit_ret", "indent_blocks"],
    "E_D+local_ann": ["unwrap_main"] + L1 + ["implicit_ret", "drop_local_ann", "indent_blocks"],
    "F_E+ret_types": ["unwrap_main"] + L1 + ["implicit_ret", "drop_local_ann", "drop_ret_types", "indent_blocks"],
    "G_F+let_less": ["unwrap_main"] + L1 + ["implicit_ret", "drop_local_ann", "drop_ret_types", "drop_let_var", "indent_blocks"],
    "H_G+param_types+positional": ["unwrap_main"] + L1 + ["implicit_ret", "drop_local_ann", "drop_ret_types", "drop_let_var",
                                                           "drop_param_types", "positional_struct", "indent_blocks"],
}

texts, index = [], {}


def want(t):
    if t not in index:
        index[t] = len(texts)
        texts.append(t)
    return index[t]


def run(src, steps):
    for st in steps:
        src = M[st](src)
    return src


plan = []
for cname, c in CORP.items():
    for task, langs in c.items():
        ny = data.strip_nyra_comments(langs["nyra"])
        py = data.strip_py_comments(langs["python"])
        row = {"corpus": cname, "task": task, "nyra": want(ny), "python": want(py), "variants": {}}
        for name in IDIOMS:
            row["variants"]["idiom:" + name] = want(M[name](ny))
        for pn, steps in PKG.items():
            row["variants"]["pkg:" + pn] = want(run(ny, steps))
        plan.append(row)

counts = count_many(texts, workers=12)
rows = [{"corpus": r["corpus"], "task": r["task"], "nyra": counts[r["nyra"]], "python": counts[r["python"]],
         "variants": {k: counts[v] for k, v in r["variants"].items()}} for r in plan]
json.dump(rows, open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "ablate2.json"), "w"), indent=1)

for cn in CORP:
    sub = [r for r in rows if r["corpus"] == cn]
    ny, py = sum(r["nyra"] for r in sub), sum(r["python"] for r in sub)
    gap = ny - py
    print(f"== {cn} n={len(sub)} nyra {ny} py {py} ratio {ny/py:.3f} gap {gap}")
    for k in rows[0]["variants"]:
        v = sum(r["variants"][k] for r in sub)
        mr = sum(r["variants"][k] / r["python"] for r in sub) / len(sub)
        print(f"   {k:34} saves {ny-v:6} tok = {100*(ny-v)/gap:5.1f}% of gap, {(ny-v)/len(sub):6.1f}/prog  -> ratio {v/py:5.3f} (mean-of-ratios {mr:.3f})")
