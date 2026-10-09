"""How much of a Python program is irreducible 'payload' (identifiers, literals) vs structure?  Output: floor.json

For each Python program (opus + sonnet + reference, comments stripped):
  full     tokens of the program
  payload  tokens of the program with every operator / bracket / keyword / indentation removed, keeping identifiers,
           numbers and string literals joined by single spaces (a generous lower bound: no syntax at all)
  names    tokens of the distinct identifiers only (each once)
  lits     tokens of numbers and string literals only
"""
import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import io, json, keyword, tokenize
import data
from tokcount import count_many

KEEP_BUILTIN = set()  # builtins like print/len/range are kept as identifiers: they would exist in any language


def split(src):
    names, lits, payload = [], [], []
    for tok in tokenize.generate_tokens(io.StringIO(src).readline):
        if tok.type == tokenize.NAME and not keyword.iskeyword(tok.string):
            names.append(tok.string); payload.append(tok.string)
        elif tok.type == tokenize.NUMBER:
            lits.append(tok.string); payload.append(tok.string)
        elif tok.type == tokenize.STRING:
            lits.append(tok.string); payload.append(tok.string)
    return names, lits, payload


corp = {"ref": data.reference(), "opus": data.model_pairs("opus"), "sonnet": data.model_pairs("sonnet")}
rows, texts = [], []
for cn, c in corp.items():
    for t, v in c.items():
        py = data.strip_py_comments(v["python"])
        try:
            names, lits, payload = split(py)
        except Exception:
            continue
        uniq = list(dict.fromkeys(names))
        row = dict(corpus=cn, task=t, full=len(texts), payload=len(texts) + 1, names=len(texts) + 2, lits=len(texts) + 3)
        texts += [py, " ".join(payload), " ".join(uniq), " ".join(lits)]
        rows.append(row)
cnt = count_many(texts)
res = []
for r in rows:
    res.append({k: (cnt[v] if k not in ("corpus", "task") else v) for k, v in r.items()})
json.dump(res, open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "floor.json"), "w"))
for cn in corp:
    sub = [r for r in res if r["corpus"] == cn]
    f = sum(r["full"] for r in sub)
    print(cn, len(sub), "full", f, "payload", sum(r["payload"] for r in sub), f"({100*sum(r['payload'] for r in sub)/f:.0f}%)",
          "distinct-names", sum(r["names"] for r in sub), "literals", sum(r["lits"] for r in sub))
