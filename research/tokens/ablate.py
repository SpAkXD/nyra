"""Gap breakdown: apply each mechanical rewrite alone (and all together) to the Nyra programs, count tokens with the
Claude tokenizer, report the saving as a share of the Nyra - Python gap.  Output: ablate.json"""
import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import json
import data
import rewrites as R
from tokcount import count_many

CORP = {"ref": data.reference(), "opus": data.model_pairs("opus"), "sonnet": data.model_pairs("sonnet")}
ORDER = ["unwrap_main", "drop_close_braces", "indent_blocks", "drop_let_var", "drop_let_only", "drop_param_types",
         "drop_ret_types", "drop_local_ann", "implicit_ret", "positional_struct"]
COMBO = {"all_syntax_braces": ["unwrap_main", "drop_param_types", "drop_ret_types", "drop_local_ann", "drop_let_var",
                               "implicit_ret", "positional_struct", "drop_close_braces"],
         "all_syntax_indent": ["unwrap_main", "drop_param_types", "drop_ret_types", "drop_local_ann", "drop_let_var",
                               "implicit_ret", "positional_struct", "indent_blocks"]}

texts = []
index = {}


def want(t):
    if t not in index:
        index[t] = len(texts)
        texts.append(t)
    return index[t]


plan = []
for cname, c in CORP.items():
    for task, langs in c.items():
        ny = data.strip_nyra_comments(langs["nyra"])
        py = data.strip_py_comments(langs["python"])
        row = {"corpus": cname, "task": task, "nyra": want(ny), "python": want(py), "variants": {}}
        for name in ORDER:
            row["variants"][name] = want(R.SYNTAX[name](ny))
        for cn, steps in COMBO.items():
            s = ny
            for st in steps:
                s = R.SYNTAX[st](s)
            row["variants"][cn] = want(s)
        plan.append(row)

counts = count_many(texts, workers=12)
rows = []
for r in plan:
    rows.append({"corpus": r["corpus"], "task": r["task"], "nyra": counts[r["nyra"]], "python": counts[r["python"]],
                 "variants": {k: counts[v] for k, v in r["variants"].items()}})
json.dump(rows, open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "ablate.json"), "w"), indent=1)

print(f"{'corpus':8} {'n':>3} {'nyra':>7} {'py':>7} {'ratio':>6} {'gap':>6}")
for cn in CORP:
    sub = [r for r in rows if r["corpus"] == cn]
    ny, py = sum(r["nyra"] for r in sub), sum(r["python"] for r in sub)
    gap = ny - py
    print(f"{cn:8} {len(sub):3} {ny:7} {py:7} {ny/py:6.2f} {gap:6}")
    for k in list(ORDER) + list(COMBO):
        v = sum(r["variants"][k] for r in sub)
        print(f"   {k:20} saves {ny-v:6} tok = {100*(ny-v)/gap:5.1f}% of gap, {(ny-v)/len(sub):6.1f}/prog  -> ratio {v/py:5.3f}")
