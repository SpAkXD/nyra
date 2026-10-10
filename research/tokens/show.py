import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import data
m, tasks = sys.argv[1], sys.argv[2:]
c = data.reference() if m == "ref" else data.model_pairs(m)
for t in tasks:
    for l in ("nyra", "python"):
        print(f"=== {m}/{t}/{l}"); print(c[t][l])
