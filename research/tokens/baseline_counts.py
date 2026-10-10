"""Token counts (Claude tokenizer, count_tokens endpoint) of every corpus program: raw and comment-stripped."""
import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import json
import sys
import time
import data
from tokcount import count_many

t0 = time.time()
corp = {"ref": data.reference()}
for m in ("opus", "sonnet"):
    corp[m] = data.model_pairs(m)
rows = []
texts = []
for cname, c in corp.items():
    for task, langs in c.items():
        for lang, code in langs.items():
            s = data.STRIP[lang](code)
            rows.append((cname, task, lang, code, s))
            texts += [code, s]
res = count_many(texts, workers=12)
out = []
for i, (cname, task, lang, code, s) in enumerate(rows):
    out.append({"corpus": cname, "task": task, "lang": lang, "raw": res[2 * i], "stripped": res[2 * i + 1]})
json.dump(out, open("baseline_counts.json", "w"))
print(len(rows), "programs", f"{time.time()-t0:.0f}s")
for cname in corp:
    for lang in data.EXT:
        sub = [r for r in out if r["corpus"] == cname and r["lang"] == lang]
        if sub:
            print(cname, lang, len(sub), "raw", sum(r["raw"] for r in sub), "stripped", sum(r["stripped"] for r in sub))
