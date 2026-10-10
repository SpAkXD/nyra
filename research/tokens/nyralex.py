"""A small Nyra lexer for source-to-source experiments (not the real compiler front end).

tokens(src) -> list of Tok(kind, text, start, end, line); kinds: ws nl comment str char num id kw op
Strings are opaque single tokens (interpolation braces are skipped with nested-quote awareness).
"""
import re
from collections import namedtuple

Tok = namedtuple("Tok", "kind text start end line")
KW = {"fn", "struct", "let", "var", "if", "else", "while", "for", "in", "step", "ret", "break", "continue", "inout",
      "ex", "true", "false", "use", "free", "keep", "arena"}
OPS2 = ["->", "=>", "==", "!=", "<=", ">=", "&&", "||", "+=", "-=", "*=", "/=", "%=", ".."]
NUM = re.compile(r"\d[\d_]*(\.\d+)?([eE][+-]?\d+)?")
ID = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")
CHAR = re.compile(r"'(\\.|[^'\\])'")


def _skip_string(s, i):
    """s[i] == '"'; returns index just after the closing quote (or end of line / input if unterminated)"""
    n = len(s)
    j = i + 1
    while j < n:
        c = s[j]
        if c == "\\":
            j += 2
            continue
        if c == '"':
            return j + 1
        if c == "{":
            if j + 1 < n and s[j + 1] == "{":
                j += 2
                continue
            k = _skip_interp(s, j)
            if k is not None:
                j = k
                continue
        if c == "\n":
            return j
        j += 1
    return n


def _skip_interp(s, i):
    """s[i] == '{' inside a string: index after the matching '}' on this line, or None if it is text"""
    n = len(s)
    depth = 0
    j = i
    while j < n:
        c = s[j]
        if c == "\n":
            return None
        if c == '"':
            k = _skip_string(s, j)
            if k > n or k == 0 or s[k - 1] != '"' or k - 1 == j:
                return None
            j = k
            continue
        if c == "'":
            m = CHAR.match(s, j)
            if m:
                j = m.end()
                continue
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                return j + 1
        j += 1
    return None


def tokens(s):
    out = []
    i, n, line = 0, len(s), 1
    while i < n:
        c = s[i]
        if c == "\n":
            out.append(Tok("nl", c, i, i + 1, line)); i += 1; line += 1
        elif c in " \t\r":
            j = i
            while j < n and s[j] in " \t\r":
                j += 1
            out.append(Tok("ws", s[i:j], i, j, line)); i = j
        elif c == "/" and s.startswith("//", i):
            j = s.find("\n", i)
            j = n if j < 0 else j
            out.append(Tok("comment", s[i:j], i, j, line)); i = j
        elif c == '"':
            j = _skip_string(s, i)
            out.append(Tok("str", s[i:j], i, j, line)); i = j
        elif c == "'":
            m = CHAR.match(s, i)
            if m:
                out.append(Tok("char", m.group(0), i, m.end(), line)); i = m.end()
            else:
                out.append(Tok("op", c, i, i + 1, line)); i += 1
        elif c.isdigit():
            m = NUM.match(s, i)
            txt = m.group(0)
            out.append(Tok("num", txt, i, i + len(txt), line)); i += len(txt)
        elif c.isalpha() or c == "_":
            m = ID.match(s, i)
            t = m.group(0)
            out.append(Tok("kw" if t in KW else "id", t, i, i + len(t), line)); i += len(t)
        else:
            for op in OPS2:
                if s.startswith(op, i):
                    out.append(Tok("op", op, i, i + len(op), line)); i += len(op); break
            else:
                out.append(Tok("op", c, i, i + 1, line)); i += 1
    return out


def sig(toks):
    """significant tokens (no ws / comments), keeping newlines"""
    return [t for t in toks if t.kind not in ("ws", "comment")]


def apply_edits(s, edits):
    """edits: list of (start, end, new); non-overlapping"""
    edits = sorted(edits, key=lambda e: (e[0], e[1]))
    out, pos = [], 0
    for a, b, new in edits:
        if a < pos:
            continue
        out.append(s[pos:a]); out.append(new); pos = b
    out.append(s[pos:])
    return "".join(out)


if __name__ == "__main__":
    import sys
    src = open(sys.argv[1], encoding="utf-8").read()
    for t in tokens(src):
        if t.kind not in ("ws", "nl"):
            print(t.kind, repr(t.text))
