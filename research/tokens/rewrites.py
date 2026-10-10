"""Mechanical Nyra -> Nyra' source rewrites, one per proposal. Each takes and returns source text.

They are *simulations*: the output is not valid Nyra 0.5, only text with the token profile a model would
write in the proposed syntax. Each rewrite is applied alone to measure its saving, then in packages.
"""
import re
from nyralex import tokens, sig, apply_edits, Tok


def _sig_idx(toks):
    return [i for i, t in enumerate(toks) if t.kind not in ("ws", "comment")]


def _match(toks, i, o, c):
    """toks[i] is the opening op `o`; index of the matching `c` (significant ops only)"""
    d = 0
    for j in range(i, len(toks)):
        t = toks[j]
        if t.kind == "op":
            if t.text == o:
                d += 1
            elif t.text == c:
                d -= 1
                if d == 0:
                    return j
    return None


def _headers(toks):
    """yield (fn_idx, name_idx, open_paren_idx, close_paren_idx, arrow_idx|None, end_idx(brace or '='))"""
    s = _sig_idx(toks)
    pos = {i: k for k, i in enumerate(s)}
    for k, i in enumerate(s):
        t = toks[i]
        if t.kind == "kw" and t.text == "fn" and k + 2 < len(s) and toks[s[k + 1]].kind == "id" and toks[s[k + 2]].text == "(":
            op = s[k + 2]
            cp = _match(toks, op, "(", ")")
            if cp is None:
                continue
            k2 = pos[cp] + 1
            arrow = None
            end = None
            if k2 < len(s) and toks[s[k2]].text == "->":
                arrow = s[k2]
                k3 = k2
                while k3 < len(s) and toks[s[k3]].text not in ("{", "="):
                    k3 += 1
                end = s[k3] if k3 < len(s) else None
            elif k2 < len(s) and toks[s[k2]].text in ("{", "=", ":"):
                end = s[k2]
            yield i, s[k + 1], op, cp, arrow, end


def drop_param_types(src):
    toks = tokens(src)
    edits = []
    for fn, name, op, cp, arrow, end in _headers(toks):
        depth = 0
        j = op + 1
        while j < cp:
            t = toks[j]
            if t.kind == "op" and t.text in "([":
                depth += 1
            elif t.kind == "op" and t.text in ")]":
                depth -= 1
            elif t.kind == "op" and t.text == ":" and depth == 0:
                k = j + 1
                d2 = 0
                last = j
                while k < cp:
                    u = toks[k]
                    if u.kind == "op" and u.text in "([":
                        d2 += 1
                    elif u.kind == "op" and u.text in ")]":
                        d2 -= 1
                    elif u.kind == "op" and u.text == "," and d2 == 0:
                        break
                    if u.kind not in ("ws",):
                        last = k
                    k += 1
                edits.append((toks[j].start, toks[last].end, ""))
                j = k
                continue
            j += 1
    return apply_edits(src, edits)


def drop_ret_types(src):
    toks = tokens(src)
    edits = []
    for fn, name, op, cp, arrow, end in _headers(toks):
        if arrow is not None and end is not None:
            # remove " -> T" (the space before the arrow too)
            a = toks[arrow].start
            while a > 0 and src[a - 1] == " ":
                a -= 1
            # keep one space before the `{`/`=`
            b = toks[end].start
            edits.append((a, b, " "))
    return apply_edits(src, edits)


def drop_local_ann(src):
    toks = tokens(src)
    s = _sig_idx(toks)
    edits = []
    for k, i in enumerate(s):
        t = toks[i]
        if t.kind == "kw" and t.text in ("let", "var") and k + 2 < len(s) and toks[s[k + 1]].kind == "id" and toks[s[k + 2]].text == ":":
            j = k + 3
            while j < len(s) and toks[s[j]].text != "=" and toks[s[j]].kind != "nl":
                j += 1
            if j < len(s) and toks[s[j]].text == "=":
                edits.append((toks[s[k + 2]].start, toks[s[j - 1]].end, ""))
    return apply_edits(src, edits)


def drop_let_var(src):
    """`let x = ` / `var x = ` -> `x = ` (first assignment declares)"""
    toks = tokens(src)
    s = _sig_idx(toks)
    edits = []
    for k, i in enumerate(s):
        t = toks[i]
        if t.kind == "kw" and t.text in ("let", "var") and (k == 0 or toks[s[k - 1]].kind == "nl" or toks[s[k - 1]].text in ("{", "}")):
            a = t.start
            b = toks[s[k + 1]].start
            edits.append((a, b, ""))
    return apply_edits(src, edits)


def drop_let_only(src):
    """`let` -> nothing, `var` kept (immutable is the default, `var` marks mutable)"""
    toks = tokens(src)
    s = _sig_idx(toks)
    edits = []
    for k, i in enumerate(s):
        t = toks[i]
        if t.kind == "kw" and t.text == "let" and (k == 0 or toks[s[k - 1]].kind == "nl" or toks[s[k - 1]].text in ("{", "}")):
            edits.append((t.start, toks[s[k + 1]].start, ""))
    return apply_edits(src, edits)


def unwrap_main(src):
    lines = src.split("\n")
    out = []
    i = 0
    while i < len(lines):
        if lines[i].rstrip() == "fn main() {":
            j = i + 1
            while j < len(lines) and lines[j].rstrip() != "}":
                j += 1
            for l in lines[i + 1:j]:
                out.append(l[4:] if l.startswith("    ") else l)
            i = j + 1
            continue
        out.append(lines[i])
        i += 1
    return "\n".join(out)


def implicit_ret(src):
    """last `ret X` of a function body (before the closing `}` at column 0) -> `X`"""
    lines = src.split("\n")
    for i in range(1, len(lines)):
        if lines[i].rstrip() == "}" and lines[i - 1].startswith("    ret "):
            lines[i - 1] = "    " + lines[i - 1][8:]
    return "\n".join(lines)


def drop_close_braces(src):
    """delete lines that hold only `}`; `} else` -> `else`"""
    out = []
    for l in src.split("\n"):
        if l.strip() == "}":
            continue
        m = re.match(r"^(\s*)\} else (.*)$", l)
        if m:
            l = m.group(1) + "else " + m.group(2)
        out.append(l)
    return "\n".join(out)


def indent_blocks(src):
    """Python-style blocks: `{` at line end -> `:`, lone `}` deleted, `} else {` -> `else:`, `} else if c {` -> `elif c:`;
    one-line `if c { a }` -> `if c: a`.  (expression `if a { x } else { y }` is left alone)"""
    toks = tokens(src)
    out_lines = []
    for l in src.split("\n"):
        st = l.strip()
        if st == "}":
            continue
        m = re.match(r"^(\s*)\} else if (.*) \{$", l)
        if m:
            out_lines.append(f"{m.group(1)}elif {m.group(2)}:")
            continue
        m = re.match(r"^(\s*)\} else \{$", l)
        if m:
            out_lines.append(f"{m.group(1)}else:")
            continue
        if l.rstrip().endswith("{") and not l.rstrip().endswith("{{"):
            ind = l[: len(l) - len(l.lstrip())]
            body = l.rstrip()[:-1].rstrip()
            if re.match(r"^\s*(fn|struct|if|else|while|for|arena)\b", l) or body.endswith(")"):
                out_lines.append(body + ":")
                continue
        # statement-level one-liner: `if c { a }`  /  `if c { a } else { b }`  /  `while c { a }`
        m = re.match(r"^(\s*)if\s(.*?)\s\{\s(.*?)\s\}\s+else\s+\{\s(.*)\s\}$", l)
        if m and "{" not in m.group(2) and "{" not in m.group(3) and "{" not in m.group(4):
            ind = m.group(1)
            out_lines += [f"{ind}if {m.group(2)}:", f"{ind}    {m.group(3)}", f"{ind}else:", f"{ind}    {m.group(4)}"]
            continue
        m = re.match(r"^(\s*)(if|while|for)\s(.*?)\s\{\s(.*)\s\}$", l)
        if m and "{" not in m.group(3):
            out_lines.append(f"{m.group(1)}{m.group(2)} {m.group(3)}: {m.group(4)}")
            continue
        out_lines.append(l)
    return "\n".join(out_lines)


STRUCT_NAMES = re.compile(r"\bstruct\s+([A-Z]\w*)")


def positional_struct(src):
    """Point(x: 1, y: 2) -> Point(1, 2) for declared struct names"""
    names = set(STRUCT_NAMES.findall(src))
    if not names:
        return src
    toks = tokens(src)
    s = _sig_idx(toks)
    edits = []
    for k, i in enumerate(s):
        t = toks[i]
        if t.kind == "id" and t.text in names and k + 1 < len(s) and toks[s[k + 1]].text == "(" and not (k and toks[s[k - 1]].text == "struct"):
            op = s[k + 1]
            cp = _match(toks, op, "(", ")")
            d = 0
            j = op + 1
            argstart = True
            while j < cp:
                u = toks[j]
                if u.kind == "op" and u.text in "([{":
                    d += 1
                elif u.kind == "op" and u.text in ")]}":
                    d -= 1
                if d == 0 and u.kind == "id" and argstart:
                    # label?
                    k2 = j + 1
                    while toks[k2].kind in ("ws",):
                        k2 += 1
                    if toks[k2].text == ":":
                        k3 = k2 + 1
                        while toks[k3].kind == "ws":
                            k3 += 1
                        edits.append((u.start, toks[k3].start, ""))
                if u.kind not in ("ws", "nl"):
                    argstart = (u.kind == "op" and u.text == "," and d == 0)
                j += 1
    return apply_edits(src, edits)


def struct_one_line_keep(src):
    return src


def strip_comments(src):
    from data import strip_nyra_comments
    return strip_nyra_comments(src)


SYNTAX = {
    "unwrap_main": unwrap_main,
    "drop_close_braces": drop_close_braces,
    "indent_blocks": indent_blocks,
    "drop_param_types": drop_param_types,
    "drop_ret_types": drop_ret_types,
    "drop_local_ann": drop_local_ann,
    "drop_let_var": drop_let_var,
    "drop_let_only": drop_let_only,
    "implicit_ret": implicit_ret,
    "positional_struct": positional_struct,
}


def rename_keywords(src, mapping=None):
    """token-level keyword renames; default fn->def, ret->return, struct->class"""
    mapping = mapping or {"fn": "def", "ret": "return", "struct": "class"}
    toks = tokens(src)
    edits = [(t.start, t.end, mapping[t.text]) for t in toks if t.kind == "kw" and t.text in mapping]
    return apply_edits(src, edits)


def kw_fn_def(src):
    return rename_keywords(src, {"fn": "def"})


def kw_ret_return(src):
    return rename_keywords(src, {"ret": "return"})


def kw_struct_class(src):
    return rename_keywords(src, {"struct": "class"})


def kw_elif(src):
    return re.sub(r"\belse if\b", "elif", src)


SYNTAX.update({"kw_fn_def": kw_fn_def, "kw_ret_return": kw_ret_return, "kw_struct_class": kw_struct_class,
               "kw_all": rename_keywords, "kw_elif": kw_elif})
