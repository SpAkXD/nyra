"""Keyword respelling alone on every corpus program (Claude tokenizer)."""
import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import json, data
import rewrites as R
from tokcount import count_many
CORP = {"ref": data.reference(), "opus": data.model_pairs("opus"), "sonnet": data.model_pairs("sonnet")}
V = ["kw_fn_def", "kw_ret_return", "kw_struct_class", "kw_elif", "kw_all"]
texts = []; plan = []
for cn, c in CORP.items():
    for t, v in c.items():
        ny = data.strip_nyra_comments(v["nyra"]); py = data.strip_py_comments(v["python"])
        r = {"cn": cn, "t": t, "n": len(texts)}; texts.append(ny)
        r["p"] = len(texts); texts.append(py)
        for k in V:
            r[k] = len(texts); texts.append(R.SYNTAX[k](ny))
        plan.append(r)
c = count_many(texts)
res = {}
for cn in CORP:
    sub = [r for r in plan if r["cn"] == cn]
    ny = sum(c[r["n"]] for r in sub); py = sum(c[r["p"]] for r in sub)
    print(cn, len(sub), "nyra", ny, "py", py, f"{ny/py:.3f}")
    for k in V:
        v = sum(c[r[k]] for r in sub)
        print(f"   {k:16} saves {ny-v:5} ({(ny-v)/len(sub):.1f}/prog, {100*(ny-v)/(ny-py):.1f}% of gap) ratio {v/py:.3f}")
