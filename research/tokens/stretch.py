"""Radical, familiarity-costing options applied to the hand-written Y sample (+ keyword respelling), to see how far below Python the
token count can go if correctness/familiarity were ignored.  Output: stretch.json"""
import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import glob, json, re
import rewrites as R, data
from nyralex import tokens, apply_edits
from tokcount import count_many
here = os.path.dirname(os.path.abspath(__file__))
tasks = sorted(os.path.basename(p)[:-5] for p in glob.glob(f"{here}/sample/Y/*.nyra"))
rd = lambda p: open(p, encoding="utf-8").read().strip("\n")

def print_noparen(s):
    return re.sub(r"^(\s*)print\((.*)\)$", r"\1print \2", s, flags=re.M)

def no_colon(s):
    return re.sub(r"^(\s*(?:if|elif|else|while|for|fn|struct|def|class)\b.*):$", r"\1", s, flags=re.M)

def one_space_indent(s):
    return re.sub(r"^((?:    )+)", lambda m: " " * (len(m.group(1)) // 4), s, flags=re.M)

def short_names(s):
    """rename user-declared identifiers (fn names, params, let/var names, for vars, struct fields not touched) to 1-2 letters"""
    toks = tokens(s)
    declared = {}
    sig = [t for t in toks if t.kind not in ("ws", "comment")]
    for i, t in enumerate(sig):
        if t.kind == "id" and i > 0:
            prev = sig[i - 1]
            if prev.kind == "kw" and prev.text in ("let", "var", "fn", "for", "inout"):
                declared.setdefault(t.text, 0)
            if prev.text == "," and i > 1 and sig[i - 2].kind == "id" and any(x.kind == "kw" and x.text in ("let", "var", "for") for x in sig[max(0, i - 6):i]):
                declared.setdefault(t.text, 0)
    # params: fn name ( a , b )
    for i, t in enumerate(sig):
        if t.kind == "kw" and t.text == "fn":
            j = i + 3
            while j < len(sig) and sig[j].text != ")":
                if sig[j].kind == "id" and sig[j + 1 - 0].text in (",", ")", ":") and sig[j - 1].text in ("(", ",", "var", "inout"):
                    declared.setdefault(sig[j].text, 0)
                j += 1
    builtin = {"print", "min", "max", "abs", "str", "int", "float", "char", "main"}
    names = [n for n in declared if n not in builtin and len(n) > 1]
    freq = {n: sum(1 for t in sig if t.kind == "id" and t.text == n) for n in names}
    order = sorted(names, key=lambda n: -freq[n])
    pool = [c for c in "abcdefghijklmnopqrstuvwxyz"] + [a + b for a in "abcdefghijklmnopqrstuvwxyz" for b in "abcdefghijklmnopqrstuvwxyz"]
    used = {t.text for t in sig if t.kind == "id"} - set(names)
    pool = [p for p in pool if p not in used]
    mp = {n: pool[i] for i, n in enumerate(order)}
    edits = [(t.start, t.end, mp[t.text]) for t in toks if t.kind == "id" and t.text in mp]
    return apply_edits(s, edits)

rows = []; texts = []
V = {"Y": lambda s: s, "Y+kw": R.rename_keywords, "Y+kw+print_noparen": lambda s: print_noparen(R.rename_keywords(s)),
     "Y+kw+no_colon": lambda s: no_colon(R.rename_keywords(s)),
     "Y+kw+print_noparen+no_colon": lambda s: no_colon(print_noparen(R.rename_keywords(s))),
     "Y+kw+short_names": lambda s: short_names(R.rename_keywords(s))}
for t in tasks:
    y = rd(f"{here}/sample/Y/{t}.nyra"); py = rd(f"{here}/sample/py/{t}.py")
    row = {"task": t, "py": len(texts)}; texts.append(py)
    for k, f in V.items():
        row[k] = len(texts); texts.append(f(y))
    rows.append(row)
c = count_many(texts)
py = sum(c[r["py"]] for r in rows)
print("python", py)
res = {}
for k in V:
    v = sum(c[r[k]] for r in rows)
    res[k] = v
    print(f"{k:34} {v:6}  ratio {v/py:.3f}")
json.dump({"python": py, **res}, open(f"{here}/stretch.json", "w"), indent=1)
if len(sys.argv) > 1:
    t = sys.argv[1]; print(short_names(R.rename_keywords(rd(f"{here}/sample/Y/{t}.nyra"))))
