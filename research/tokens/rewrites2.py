"""Library / idiom rewrites (mechanical where a pattern is clear). Same conventions as rewrites.py."""
import re
from nyralex import tokens, apply_edits
from rewrites import _sig_idx, _match

# ---- negative index -------------------------------------------------------------------------------------------

NEG = re.compile(r"(?<![\w.])([A-Za-z_]\w*(?:\.\w+)*)\[\1\.len\(\) - (\d+)\]")


def neg_index(src):
    return NEG.sub(lambda m: f"{m.group(1)}[-{m.group(2)}]", src)


# ---- `x in xs` instead of `xs.contains(x)` ---------------------------------------------------------------------

CONT = re.compile(r"(!?)((?<![\w.])[A-Za-z_]\w*(?:\.\w+)*)\.(?:contains|has)\(")


def in_op(src):
    out, pos = [], 0
    for m in CONT.finditer(src):
        if m.start() < pos:
            continue
        # balanced argument
        i = m.end()
        d = 1
        j = i
        instr = False
        while j < len(src) and d:
            c = src[j]
            if c == '"':
                # skip string
                from nyralex import _skip_string
                j = _skip_string(src, j)
                continue
            if c == "(":
                d += 1
            elif c == ")":
                d -= 1
            j += 1
        arg = src[i:j - 1]
        if d or "," in arg and "(" not in arg:
            continue
        out.append(src[pos:m.start()])
        out.append(f"{arg} not in {m.group(2)}" if m.group(1) else f"{arg} in {m.group(2)}")
        pos = j
    out.append(src[pos:])
    return "".join(out)


# ---- python-style conditional expression -----------------------------------------------------------------------

def ternary(src):
    toks = tokens(src)
    s = _sig_idx(toks)
    edits = []
    pos = {i: k for k, i in enumerate(s)}
    for k, i in enumerate(s):
        t = toks[i]
        if not (t.kind == "kw" and t.text == "if" and k > 0):
            continue
        prev = toks[s[k - 1]]
        if prev.kind == "nl" or prev.text in ("{", "}") or (prev.kind == "kw" and prev.text == "else"):
            continue
        # find `{` (the first top-level `{` after the condition)
        j = k + 1
        d = 0
        while j < len(s):
            u = toks[s[j]]
            if u.text in ("(", "["):
                d += 1
            elif u.text in (")", "]"):
                d -= 1
            elif u.text == "{" and d == 0:
                break
            elif u.kind == "nl":
                j = None
                break
            j += 1
        if j is None or j >= len(s):
            continue
        ob = s[j]
        cb = _match(toks, ob, "{", "}")
        if cb is None or any(x.kind == "nl" for x in toks[ob:cb]):
            continue
        # else
        e = pos.get(cb)
        nxt = s[e + 1] if e is not None and e + 1 < len(s) else None
        if nxt is None or not (toks[nxt].kind == "kw" and toks[nxt].text == "else"):
            continue
        ob2i = s[e + 2]
        if toks[ob2i].text != "{":
            continue
        cb2 = _match(toks, ob2i, "{", "}")
        if cb2 is None or any(x.kind == "nl" for x in toks[ob2i:cb2]):
            continue
        cond = src[toks[s[k + 1]].start: toks[ob].start].strip()
        a = src[toks[ob].end: toks[cb].start].strip()
        b = src[toks[ob2i].end: toks[cb2].start].strip()
        edits.append((t.start, toks[cb2].end, f"{a} if {cond} else {b}"))
    return apply_edits(src, edits)


# ---- string building: "a" + str(x) + "b"  ->  "a{x}b" -----------------------------------------------------------

SPLIT_OPS = {",", "==", "!=", "<", ">", "<=", ">=", "&&", "||", "=", "+=", "-=", "*=", "/=", "%=", "->", "=>", "..", ":", "!"}
SPLIT_KW = {"in", "if", "else", "ret", "step", "while", "for", "let", "var"}


def _segments(toks):
    """yield lists of (start_tok, end_tok) for each term of every `+` chain, per nesting frame"""
    s = _sig_idx(toks)
    stack = [[]]  # each frame: list of segments; a segment is a list of terms; a term is [first, last] token idx
    cur = [[None, None]]  # not used; we rebuild below
    frames = [{"segs": [[[]]], "bad": [False], "k": "top"}]
    results = []

    def close_seg(fr):
        seg = fr["segs"][-1]
        if len(seg) >= 2 and all(t for t in seg) and not fr["bad"][-1]:
            results.append([(t[0], t[-1]) for t in seg])
        fr["segs"].append([[]])
        fr["bad"].append(False)

    for i in s:
        t = toks[i]
        fr = frames[-1]
        if t.kind == "nl":
            if fr["k"] in ("top", "{"):
                close_seg(fr)
            continue
        if t.kind == "op" and t.text in ("(", "[", "{"):
            fr["segs"][-1][-1].append(i)
            frames.append({"segs": [[[]]], "bad": [False], "k": t.text})
            continue
        if t.kind == "op" and t.text in (")", "]", "}"):
            close_seg(fr)
            frames.pop()
            frames[-1]["segs"][-1][-1].append(i)
            continue
        if (t.kind == "op" and t.text in SPLIT_OPS) or (t.kind == "kw" and t.text in SPLIT_KW):
            close_seg(fr)
            continue
        if t.kind == "op" and t.text == "+":
            fr["segs"][-1].append([])
            continue
        if t.kind == "op" and t.text == "-":
            fr["bad"][-1] = True
        fr["segs"][-1][-1].append(i)
    close_seg(frames[0])
    return results


def interp(src):
    toks = tokens(src)
    edits = []
    for seg in _segments(toks):
        texts = [(toks[a].kind == "str" and a == b, src[toks[a].start: toks[b].end]) for a, b in seg]
        if not any(lit for lit, _ in texts):
            continue
        if any(toks[a].kind == "char" for a, b in seg):
            continue
        parts = []
        for (a, b), (lit, txt) in zip(seg, texts):
            if lit:
                parts.append(txt[1:-1])
            else:
                inner = txt
                if txt.startswith("str(") and txt.endswith(")"):
                    tt = tokens(txt)
                    if _match(tt, 1, "(", ")") == len(tt) - 1:
                        inner = txt[4:-1]
                parts.append("{" + inner + "}")
        edits.append((toks[seg[0][0]].start, toks[seg[-1][1]].end, '"' + "".join(parts) + '"'))
    return apply_edits(src, edits)


PADL1 = re.compile(r"\{str\(([^{}]*?)\)\.pad_left\((\d+)(?:, '(.)')?\)\}")
PADL2 = re.compile(r"\{([^{}]*?)\.pad_left\((\d+)(?:, '(.)')?\)\}")
PADR1 = re.compile(r"\{str\(([^{}]*?)\)\.pad_right\((\d+)\)\}")
PADR2 = re.compile(r"\{([^{}]*?)\.pad_right\((\d+)\)\}")


def pad_spec(src):
    """inside an interpolation: `{x.pad_left(3)}` -> `{x:>3}`, `{x.pad_right(3)}` -> `{x:<3}`, `pad_left(3, '0')` -> `{x:03}`"""
    def l(m):
        if m.group(3):
            return "{" + m.group(1) + ":" + m.group(3) + m.group(2) + "}" if m.group(3) == "0" else "{" + m.group(1) + ":" + m.group(3) + ">" + m.group(2) + "}"
        return "{" + m.group(1) + ":>" + m.group(2) + "}"

    def r(m):
        return "{" + m.group(1) + ":<" + m.group(2) + "}"
    for rx in (PADL1, PADL2):
        src = rx.sub(l, src)
    for rx in (PADR1, PADR2):
        src = rx.sub(r, src)
    return src


# ---- bare function as lambda --------------------------------------------------------------------------------------

BARE = re.compile(r"\b(\w+) => ([A-Za-z_]\w*)\(\1\)")


def bare_lambda(src):
    return BARE.sub(lambda m: m.group(2), src)


# ---- accumulate loop -> comprehension -----------------------------------------------------------------------------

def comprehension(src):
    lines = src.split("\n")
    out = []
    i = 0
    decl = re.compile(r"^(\s*)(?:var|let) (\w+)(?:: [^=]+)? = \[\]$")
    while i < len(lines):
        m = decl.match(lines[i])
        if m and i + 3 < len(lines):
            ind, name = m.group(1), m.group(2)
            f = re.match(rf"^{ind}for (\w+(?:, \w+)?) in (.+) \{{$", lines[i + 1])
            if f:
                # form A: single push
                p = re.match(rf"^{ind}    {name}\.push\((.+)\)$", lines[i + 2])
                if p and lines[i + 3] == f"{ind}}}":
                    out.append(f"{ind}var {name} = [{p.group(1)} for {f.group(1)} in {f.group(2)}]")
                    i += 4
                    continue
                # form B: if C { push }
                p = re.match(rf"^{ind}    if (.+) \{{ {name}\.push\((.+)\) \}}$", lines[i + 2])
                if p and "{" not in p.group(1) and "}" not in p.group(2) and lines[i + 3] == f"{ind}}}":
                    out.append(f"{ind}var {name} = [{p.group(2)} for {f.group(1)} in {f.group(2)} if {p.group(1)}]")
                    i += 4
                    continue
                if i + 5 < len(lines):
                    p = re.match(rf"^{ind}    if (.+) \{{$", lines[i + 2])
                    q = re.match(rf"^{ind}        {name}\.push\((.+)\)$", lines[i + 3])
                    if p and q and lines[i + 4] == f"{ind}    }}" and lines[i + 5] == f"{ind}}}":
                        out.append(f"{ind}var {name} = [{q.group(1)} for {f.group(1)} in {f.group(2)} if {p.group(1)}]")
                        i += 6
                        continue
        out.append(lines[i])
        i += 1
    return "\n".join(out)


IDIOM = {
    "neg_index": neg_index,
    "in_op": in_op,
    "ternary": ternary,
    "interp": interp,
    "pad_spec": pad_spec,
    "bare_lambda": bare_lambda,
    "comprehension": comprehension,
}
