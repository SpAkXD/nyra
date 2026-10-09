"""Token cost of single keywords/operators in a line-start or inline context (Claude tokenizer): 10 copies of a one-line
context minus 10 copies of a context with a 1-token placeholder (`x`)."""
import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import json
from tokcount import count_many

def rep(tmpl, kw, n=10):
    return "\n".join(tmpl.format(kw=kw) for _ in range(n))

# (group, template with {kw}, alternatives)
G = [
 ("statement-start keyword", "    {kw} total = a + b", ["let", "var", "const", "val", "mut", "my", "set"], "x"),
 ("function keyword", "{kw} area(r) {{", ["fn", "def", "func", "fun", "function", "proc", "sub"], "x"),
 ("return keyword", "    {kw} total + 1", ["ret", "return", "yield", "out", "give"], "x"),
 ("struct keyword", "{kw} Point {{", ["struct", "class", "type", "record", "data"], "x"),
 ("else-if", "    }} {kw} x > 3 {{", ["else if", "elif", "elsif", "elseif"], "x"),
 ("logical and", "    if a {kw} b {{", ["&&", "and", "&"], "x"),
 ("logical or", "    if a {kw} b {{", ["||", "or", "|"], "x"),
 ("logical not", "    if {kw}done {{", ["!", "not ", "~"], ""),
 ("lambda arrow", "    xs.map(x {kw} x * 2)", ["=>", "->", ":"], "+"),
 ("range", "    for i in 0{kw}n {{", ["..", "...", ":", " to "], "+"),
 ("inout", "    f({kw} x)", ["inout", "ref", "&mut", "&", "mut"], "x"),
 ("not-equal", "    if a {kw} b {{", ["!=", "<>", "~="], "x"),
 ("loop keyword", "    {kw} x in xs {{", ["for", "foreach", "each"], "x"),
 ("block-open", "    if a > b {kw}", ["{{", ":", " then", " do"], "x"),
 ("example kw", "{kw} sq(3) == 9", ["ex", "assert", "test", "example", "check"], "x"),
 ("import kw", "{kw} math", ["use", "import", "include", "require", "from"], "x"),
 ("true", "    done = {kw}", ["true", "True", "yes", "1"], "x"),
 ("none-loop", "    {kw}", ["break", "continue", "next", "skip", "stop"], "x"),
 ("len call", "    n = {kw}", ["xs.len()", "len(xs)", "xs.length", "xs.size()", "xs.count", "#xs", "xs.n"], "x"),
 ("append", "    {kw}", ["xs.push(v)", "xs.append(v)", "xs.add(v)", "xs += [v]", "xs << v", "xs.push_back(v)"], "x"),
 ("pad", "    {kw}", ["s.pad_left(5)", "s.rjust(5)", "s.lpad(5)", "s.padl(5)", "f\"{{s:>5}}\""], "x"),
 ("to string", "    t = {kw}", ["str(n)", "n.str()", "n.to_string()", "string(n)", "n.toString()", "String(n)"], "x"),
 ("char code", "    t = {kw}", ["c.code()", "ord(c)", "c.ord()", "int(c)", "c.charCodeAt(0)"], "x"),
 ("min/max", "    t = {kw}", ["min(a, b)", "a.min(b)", "xs.min()", "min(xs)"], "x"),
 ("char literal", "    t = {kw}", ["'a'", '"a"'], "x"),
 ("split", "    t = {kw}", ['s.split(" ")', "s.split()", "s.words()", 's.split(",")'], "x"),
 ("trim", "    t = {kw}", ["s.trim()", "s.strip()"], "x"),
 ("upper", "    t = {kw}", ["s.upper()", "s.to_upper()", "s.toUpperCase()", "s.uppercase()"], "x"),
 ("starts", "    t = {kw}", ["s.starts_with(p)", "s.startswith(p)", "s.startsWith(p)", "s.has_prefix(p)"], "x"),
 ("index_of", "    t = {kw}", ["s.index_of(p)", "s.find(p)", "s.indexOf(p)", "s.index(p)"], "x"),
 ("is_digit", "    t = {kw}", ["c.is_digit()", "c.isdigit()", "c.isDigit()", "c.is_numeric()"], "x"),
 ("sort_by", "    {kw}", ["xs.sort_by(x => x.k)", "xs.sort(key=lambda x: x.k)", "xs.sort_by_key(|x| x.k)", "xs.sortBy(x => x.k)", "xs.sort(x => x.k)"], "x"),
 ("repeat", "    t = {kw}", ['s.repeat(n)', 's * n', 's.times(n)'], "x"),
 ("join", "    t = {kw}", ['xs.join(", ")', '", ".join(xs)', 'xs.join(",")'], "x"),
]
rows = []
texts = []
for g, tmpl, alts, ph in G:
    base = rep(tmpl, ph)
    texts.append(base)
    for a in alts:
        texts.append(rep(tmpl, a))
c = count_many(texts)
i = 0
out = []
for g, tmpl, alts, ph in G:
    base = c[i]; i += 1
    print(f"== {g}   (template {tmpl.strip()!r}, placeholder {ph!r} = {base/10:.1f} tokens/line)")
    for a in alts:
        n = c[i]; i += 1
        d = (n - base) / 10
        print(f"     {a!r:34} {n/10:5.1f} tokens/line   {d:+.1f} vs placeholder")
        out.append({"group": g, "alt": a, "per_line": n / 10, "delta_vs_placeholder": d})
json.dump(out, open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "keywords2.json"), "w"), indent=1)
