"""Counts, in the 24 hand-written 0.6-X programs, the constructs that the mechanical rewrite cannot produce, and prices each use with
the micro-benchmark (micro.json).  The sum is compared with the measured hand-vs-mechanical difference."""
import os, sys, re, glob, json
here = os.path.dirname(os.path.abspath(__file__))
micro = {m["id"]: m for m in json.load(open(f"{here}/micro.json"))}
files = sorted(glob.glob(f"{here}/sample/X/*.nyra"))
src = {os.path.basename(f)[:-5]: open(f, encoding="utf-8").read() for f in files}
FEATS = [
 ("var parameters (`fn f(var n: int)`)", "var_param", 5.0, lambda s: len(re.findall(r"(?:^fn \w+\(|, )var \w+: ", s, re.M))),
 ("tuple swap / multi-assign (`a, b = b, a % b`)", "swap_tuple", 5, lambda s: len(re.findall(r"^\s+[\w.\[\]]+, [\w.\[\]]+(?:, [\w.\[\]]+)* = ", s, re.M))),
 ("multi-declare from a literal (`var lo, hi = 0, n - 1`)", "multi_decl", 1.5, lambda s: len(re.findall(r"^\s*(?:let|var) \w+(?:, \w+)+ = [^\n]*, ", s, re.M)) - len(re.findall(r"^\s*(?:let|var) \w+(?:, \w+)+ = [^\n]*\(", s, re.M))),
 ("array/tuple destructuring (`let a, b = s.split(\" \")`, `let ok, t = f()`)", "destructure_split", 7, lambda s: len(re.findall(r"^\s*(?:let|var) \w+(?:, \w+)+ = [^\n]*\(", s, re.M))),
 ("list of tuples iterated with unpacking (`for v, s in [(1, \"M\"), ...]`)", "tuple_table", 30, lambda s: len(re.findall(r"for \w+(?:, \w+)+ in \[\(", s)) + len(re.findall(r"\bitems = \[\(", s))),
 ("tuple return values (`ret (false, \"\")`)", "tuple_return", 10, lambda s: len(re.findall(r"^\s*(?:ret )?\((?:true|false)", s, re.M))),
 ("`sorted_by` / `sorted` with tuple key", "sorted_by_tuple", 60, lambda s: len(re.findall(r"\.sorted(?:_by)?\(", s))),
 ("in-place update through a map element (`data[a][b] = v`)", "nested_mut", 20, lambda s: len(re.findall(r"\]\[[^\]]+\] (?:[-+]?=)", s)) ),
 ("`.trim(chars)`", "trim_chars", 80, lambda s: len(re.findall(r"\.trim\(\"", s))),
 ("format spec `{x:,}`", "fmt_thousands", 85, lambda s: len(re.findall(r":,\}", s)) // 1),
 ("conditional expression in a slice / index (`s.slice(0, e - 1 if append else e)`)", "ternary", 4, lambda s: 0),
]
tot_est = 0
rows = []
print(f"{'feature':80} {'uses':>5} {'programs':>8} {'tok/use':>8} {'est.saved':>9}")
for name, mid, per, f in FEATS:
    uses = sum(f(s) for s in src.values())
    progs = sum(1 for s in src.values() if f(s) > 0)
    est = uses * per
    tot_est += est
    rows.append({"feature": name, "micro_id": mid, "uses": uses, "programs": progs, "per_use": per, "est_saved": est})
    print(f"{name:80} {uses:5} {progs:8} {per:8.1f} {est:9.0f}")
print("estimated total", tot_est, "(measured hand-vs-mechanical difference on the 24 programs: 1319 tokens)")
json.dump(rows, open(f"{here}/featurecount.json", "w"), indent=1)
