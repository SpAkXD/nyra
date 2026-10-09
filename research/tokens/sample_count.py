"""Counts the 24-program hand-rewritten sample: orig Nyra 0.5 (Opus), Python, X start (mechanical), X (hand), Y (X + no param types,
no let/var keyword, positional constructors)."""
import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import json, glob
import rewrites as R
from tokcount import count_many

here = os.path.dirname(os.path.abspath(__file__))
tasks = sorted(os.path.basename(p)[:-5] for p in glob.glob(f"{here}/sample/X/*.nyra"))


def rd(p):
    return open(p, encoding="utf-8").read().strip("\n")


def y_from_x(x):
    for f in ("drop_param_types", "drop_let_var", "positional_struct"):
        x = R.SYNTAX[f](x)
    return x


rows = []
texts = []
for t in tasks:
    orig = rd(f"{here}/sample/orig/{t}.nyra")
    py = rd(f"{here}/sample/py/{t}.py")
    x = rd(f"{here}/sample/X/{t}.nyra")
    y = y_from_x(x)
    os.makedirs(f"{here}/sample/Y", exist_ok=True)
    open(f"{here}/sample/Y/{t}.nyra", "w", encoding="utf-8", newline="\n").write(y + "\n")
    import hview
    mh = hview.hform(orig)
    L1 = ["neg_index", "in_op", "ternary", "interp", "pad_spec", "bare_lambda", "comprehension"]
    mf = hview.hform(orig, ["unwrap_main"] + L1 + ["implicit_ret", "drop_local_ann", "drop_ret_types", "indent_blocks"])
    rows.append((t, orig, py, x, y, mh, mf))
    texts += [orig, py, x, y, mh, mf]
c = count_many(texts)
out = []
tot = [0, 0, 0, 0]
print(f"{'task':18} {'orig':>6} {'X':>6} {'Y':>6} {'py':>6}  X/py   Y/py")
tot = [0] * 6
for i, (t, *_ ) in enumerate(rows):
    o, p, x, y, mh, mf = c[6 * i:6 * i + 6]
    out.append({"task": t, "orig": o, "py": p, "X": x, "Y": y, "mechH": mh, "mechF": mf})
    for k, v in enumerate((o, p, x, y, mh, mf)):
        tot[k] += v
    print(f"{t:18} {o:6} {x:6} {y:6} {p:6}  {x/p:5.2f} {y/p:5.2f}")
o, p, x, y, mh, mf = tot
print(f"{'TOTAL':18} {o:6} {x:6} {y:6} {p:6}  {x/p:5.2f} {y/p:5.2f}   orig/py {o/p:.3f}  mechH {mh} ({mh/p:.3f}) mechF {mf} ({mf/p:.3f})")
json.dump(out, open(f"{here}/sample_counts.json", "w"), indent=1)
