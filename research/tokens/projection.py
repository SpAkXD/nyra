"""Projected Nyra/Python token ratios per package, from packages.json (mechanical, per corpus) and the hand-written 24-program sample
(factor = hand-written tokens / mechanical tokens on the same programs).  No API calls."""
import json, os
here = os.path.dirname(os.path.abspath(__file__))
P = json.load(open(f"{here}/packages.json"))
S, SM = P["sample"], P["sample_mech"]
fX = S["Xkw"] / SM["mechXkw"]
fY = S["Ykw"] / SM["mechYkw"]
KW_ALONE = {"opus": 317, "sonnet": 277, "ref": 558}  # kwablate.py, kw respelling alone
print(f"hand/mechanical factor on the sample: X {fX:.4f}  Y {fY:.4f}  (sample: orig {S['orig']/S['py']:.3f}, X {S['Xkw']/S['py']:.3f}, Y {S['Ykw']/S['py']:.3f} of Python)")
out = {}
for cn, c in P["corpora"].items():
    py, st, kw = c["python"], c["stages"], c["kw_stages"]
    # stages: 0 orig, 1 script, 2 lib, 3 impl ret, 4 local ann, 5 ret types, 6 indent (=X), 7 X marker, 8 let, 9 params, 10 positional, 11 Y marker
    rows = {
        "current": st[0] / py,
        "P0 docs-only (script)": st[1] / py,
        "P1 lite (script + respell + library idioms)": (st[2] - KW_ALONE[cn]) / py,
        "P2 0.6-X mechanical": kw[0] / py,
        "P2 0.6-X projected (mechanical x hand factor)": kw[0] * fX / py,
        "P2 0.6-X projected, half the hand factor": kw[0] * (1 + fX) / 2 / py,
        "P3 0.6-Y mechanical": kw[1] / py,
        "P3 0.6-Y projected": kw[1] * fY / py,
        "P3 0.6-Y projected, half the hand factor": kw[1] * (1 + fY) / 2 / py,
    }
    out[cn] = rows
json.dump(out, open(f"{here}/projection.json", "w"), indent=1)
names = list(next(iter(out.values())))
print(f"{'package':50}" + "".join(f"{cn:>9}" for cn in out))
for n in names:
    print(f"{n:50}" + "".join(f"{out[cn][n]:9.3f}" for cn in out))
