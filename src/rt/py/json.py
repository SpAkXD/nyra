

# ---- json: `json.str(v)` and `json.parse(text)` by the value's type ----
# A type is "i" int, "f" float, "b" bool, "c" char, "s" str, ("a", T) an array of T, ("m", K, V) a map,
# or the class of a struct, tuple, optional or enum (its `ny_jf` lists (JSON name, attribute, type) of
# every field, in order; `ny_jk` is "t" for a tuple, "o" for an optional, "e" for an enum, which has
# `ny_jn` its name and `ny_jv` (variant name, indexes of the fields that hold its values) per variant).

NY_JESC = {8: "\\b", 12: "\\f", 10: "\\n", 13: "\\r", 9: "\\t"}


def ny_jenc_str(s):
    o = ['"']
    for ch in s:
        c = ord(ch)
        if ch == '"':
            o.append('\\"')
        elif ch == "\\":
            o.append("\\\\")
        elif c < 0x20:
            o.append(NY_JESC.get(c) or "\\u%04x" % c)
        else:
            o.append(ch)
    o.append('"')
    return "".join(o)


def ny_jenc(v, d):
    if isinstance(d, type):
        k = getattr(d, "ny_jk", "")
        fs = d.ny_jf
        if k == "t":
            return "[" + ",".join(ny_jenc(getattr(v, a), t) for n, a, t in fs) + "]"
        if k == "o":
            return ny_jenc(getattr(v, fs[1][1]), fs[1][2]) if getattr(v, fs[0][1]) else "null"
        if k == "e":
            name, idx = d.ny_jv[getattr(v, fs[0][1])]
            if not idx:
                return ny_jenc_str(name)
            return "{" + ny_jenc_str(name) + ":[" + ",".join(ny_jenc(getattr(v, fs[i][1]), fs[i][2]) for i in idx) + "]}"
        return "{" + ",".join(ny_jenc_str(n) + ":" + ny_jenc(getattr(v, a), t) for n, a, t in fs) + "}"
    if isinstance(d, tuple) and d[0] == "m":
        if d[1] == "s":
            return "{" + ",".join(ny_jenc_str(k) + ":" + ny_jenc(x, d[2]) for k, x in v.items()) + "}"
        return "[" + ",".join("[" + ny_jenc(k, d[1]) + "," + ny_jenc(x, d[2]) + "]" for k, x in v.items()) + "]"
    if isinstance(d, tuple):
        return "[" + ",".join(ny_jenc(x, d[1]) for x in v) + "]"
    if d == "i":
        return str(v)
    if d == "f":
        return ny_num(v) if math.isfinite(v) else "null"
    if d == "b":
        return "true" if v else "false"
    return ny_jenc_str(v)


class NyJP:
    """The parser: the text, the position, the nesting and the path to the value being read."""

    def __init__(self, s, line, col):
        self.s, self.i, self.depth, self.path, self.line, self.col = s, 0, 0, [], line, col


def ny_jsyntax(p, what):
    line = p.s.count("\n", 0, p.i) + 1
    ny_panic("E0345", f"json.parse: invalid JSON at line {line}: {what}",
             "check the JSON text: it must be one value, with keys and strings in double quotes", p.line, p.col)


def ny_jpath(p):
    return "$" + "".join(f"[{seg}]" if isinstance(seg, int) else "." + seg for seg in p.path)


def ny_jtype(p, what):
    ny_panic("E0345", f"json.parse: expected {what} at {ny_jpath(p)}",
             "the JSON text must have the shape of the type it is read into", p.line, p.col)


def ny_jws(p):
    s, n = p.s, len(p.s)
    while p.i < n and s[p.i] in " \t\n\r":
        p.i += 1


def ny_jstart(p):
    """Skips white space to the start of a value: a syntax error unless one can start here."""
    ny_jws(p)
    if p.i >= len(p.s):
        ny_jsyntax(p, "unexpected end of the text")
    c = p.s[p.i]
    if c not in "{[\"tfn-0123456789":
        ny_jsyntax(p, "expected a value")
    return c


def ny_jopen(p, open_, what):
    """`[` or `{`: True if a first element follows, False for an empty one (already closed)."""
    if ny_jstart(p) != open_:
        ny_jtype(p, what)
    p.depth += 1
    if p.depth > 500:
        ny_jsyntax(p, "nested too deeply")
    p.i += 1
    ny_jws(p)
    close = "]" if open_ == "[" else "}"
    if p.i < len(p.s) and p.s[p.i] == close:
        p.i += 1
        p.depth -= 1
        return False
    return True


def ny_jnext(p, close):
    """After an element: True at `,` (another one follows), False at the closing bracket."""
    ny_jws(p)
    c = p.s[p.i] if p.i < len(p.s) else ""
    if c == ",":
        p.i += 1
        return True
    if c == close:
        p.i += 1
        p.depth -= 1
        return False
    ny_jsyntax(p, "unexpected end of the text" if c == "" else f"expected `,` or `{close}`")


NY_JSIMPLE = {'"': '"', "\\": "\\", "/": "/", "b": "\b", "f": "\f", "n": "\n", "r": "\r", "t": "\t"}
NY_JHEX = re.compile(r"[0-9a-fA-F]{4}")


def ny_jhex(p, at):
    h = p.s[at:at + 4]
    return int(h, 16) if NY_JHEX.fullmatch(h) else -1


def ny_jstring(p):
    """A string; the position is at its `"`."""
    s, n = p.s, len(p.s)
    out = []
    p.i += 1
    while True:
        if p.i >= n:
            ny_jsyntax(p, "unterminated string")
        c = s[p.i]
        if c == '"':
            p.i += 1
            return "".join(out)
        if ord(c) < 0x20:
            ny_jsyntax(p, "control character in a string")
        if c != "\\":
            out.append(c)
            p.i += 1
            continue
        p.i += 1
        if p.i >= n:
            ny_jsyntax(p, "unterminated string")
        e = s[p.i]
        if e in NY_JSIMPLE:
            out.append(NY_JSIMPLE[e])
            p.i += 1
            continue
        if e != "u":
            ny_jsyntax(p, "invalid escape")
        cp = ny_jhex(p, p.i + 1)
        if cp < 0 or 0xDC00 <= cp <= 0xDFFF:
            ny_jsyntax(p, "invalid escape")
        p.i += 5
        if 0xD800 <= cp <= 0xDBFF:
            lo = ny_jhex(p, p.i + 2) if s[p.i:p.i + 2] == "\\u" else -1
            if lo < 0xDC00 or lo > 0xDFFF:
                ny_jsyntax(p, "invalid escape")
            cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00)
            p.i += 6
        out.append(chr(cp))


def ny_jnumber(p):
    """A number: its text, and whether it has no fraction and no exponent."""
    s, n, start = p.s, len(p.s), p.i

    def digit():
        return p.i < n and "0" <= s[p.i] <= "9"

    whole = True
    if p.i < n and s[p.i] == "-":
        p.i += 1
    if p.i < n and s[p.i] == "0":
        p.i += 1
    elif digit():
        while digit():
            p.i += 1
    else:
        ny_jsyntax(p, "invalid number")
    if p.i < n and s[p.i] == ".":
        p.i += 1
        if not digit():
            ny_jsyntax(p, "invalid number")
        while digit():
            p.i += 1
        whole = False
    if p.i < n and s[p.i] in "eE":
        p.i += 1
        if p.i < n and s[p.i] in "+-":
            p.i += 1
        if not digit():
            ny_jsyntax(p, "invalid number")
        while digit():
            p.i += 1
        whole = False
    return s[start:p.i], whole


def ny_jliteral(p):
    for w in ("true", "false", "null"):
        if p.s.startswith(w, p.i):
            p.i += len(w)
            return w
    ny_jsyntax(p, "expected a value")


def ny_jkey(p):
    """An object's key and the `:` after it."""
    ny_jws(p)
    if p.i >= len(p.s):
        ny_jsyntax(p, "unexpected end of the text")
    if p.s[p.i] != '"':
        ny_jsyntax(p, "expected a string key")
    k = ny_jstring(p)
    ny_jws(p)
    if p.i >= len(p.s):
        ny_jsyntax(p, "unexpected end of the text")
    if p.s[p.i] != ":":
        ny_jsyntax(p, "expected `:`")
    p.i += 1
    return k


def ny_jskip(p):
    """Any value, only checked (an object's fields that the type does not have)."""
    c = ny_jstart(p)
    if c in "{[":
        if ny_jopen(p, c, ""):
            while True:
                if c == "{":
                    ny_jkey(p)
                ny_jskip(p)
                if not ny_jnext(p, "}" if c == "{" else "]"):
                    break
    elif c == '"':
        ny_jstring(p)
    elif c == "-" or "0" <= c <= "9":
        ny_jnumber(p)
    else:
        ny_jliteral(p)


def ny_jdef(d):
    """The value a type holds when nothing was read into it (an unused slot of an enum, the `val` of a `none`)."""
    if isinstance(d, type):
        return d(*[ny_jdef(f[2]) for f in d.ny_jf])
    if isinstance(d, tuple):
        return NyDict() if d[0] == "m" else NyList()
    return {"s": "", "c": "\x00", "b": False, "f": 0.0}.get(d, 0)


def ny_jfixed(p, n, each):
    """An array of exactly n elements: `each(i)` reads the i-th one (its path is already set)."""
    what = f"an array of {n} elements"
    if not ny_jopen(p, "[", what):
        ny_jtype(p, what)
    i = 0
    while True:
        if i >= n:
            ny_jtype(p, what)
        p.path.append(i)
        each(i)
        p.path.pop()
        i += 1
        if not ny_jnext(p, "]"):
            break
    if i < n:
        ny_jtype(p, what)


def ny_jenum(p, d, c):
    fs, vs = d.ny_jf, d.ny_jv

    def bad():
        ny_jtype(p, "a variant of " + d.ny_jn + ': a name, or {"Name": [values]}')

    def make(v, vals):
        a = [ny_jdef(f[2]) for f in fs]
        a[0] = v
        for j, k in enumerate(vs[v][1]):
            a[k] = vals[j]
        return d(*a)

    if c == '"':
        name = ny_jstring(p)
        v = next((i for i, x in enumerate(vs) if x[0] == name and not x[1]), -1)
        if v < 0:
            bad()
        return make(v, [])
    if c != "{" or not ny_jopen(p, "{", ""):
        bad()
    name = ny_jkey(p)
    v = next((i for i, x in enumerate(vs) if x[0] == name and x[1]), -1)
    if v < 0:
        bad()
    idx = vs[v][1]
    vals = [None] * len(idx)

    def read(i):
        vals[i] = ny_jdec(p, fs[idx[i]][2])

    p.path.append(name)
    ny_jfixed(p, len(idx), read)
    p.path.pop()
    if ny_jnext(p, "}"):
        bad()
    return make(v, vals)


def ny_jdec(p, d):
    c = ny_jstart(p)
    k = getattr(d, "ny_jk", "") if isinstance(d, type) else ""
    if k == "e":
        return ny_jenum(p, d, c)
    if k == "o":
        if c == "n":
            ny_jliteral(p)
            return d(False, ny_jdef(d.ny_jf[1][2]))
        return d(True, ny_jdec(p, d.ny_jf[1][2]))
    if k == "t":
        vals = [None] * len(d.ny_jf)

        def read(i):
            vals[i] = ny_jdec(p, d.ny_jf[i][2])

        ny_jfixed(p, len(vals), read)
        return d(*vals)
    if isinstance(d, tuple) and d[0] == "m":
        m = NyDict()
        if d[1] == "s":
            if ny_jopen(p, "{", "an object"):
                while True:
                    key = ny_jkey(p)
                    p.path.append(key)
                    m[key] = ny_jdec(p, d[2])
                    p.path.pop()
                    if not ny_jnext(p, "}"):
                        break
        elif ny_jopen(p, "[", "an array"):
            n = 0
            while True:
                pair = [None, None]

                def read(i):
                    pair[i] = ny_jdec(p, d[1 + i])

                p.path.append(n)
                n += 1
                ny_jfixed(p, 2, read)
                p.path.pop()
                m[pair[0]] = pair[1]
                if not ny_jnext(p, "]"):
                    break
        return m
    if isinstance(d, type):
        fs = d.ny_jf
        vals, seen = [None] * len(fs), [False] * len(fs)
        if ny_jopen(p, "{", "an object"):
            while True:
                k = ny_jkey(p)
                f = next((i for i, x in enumerate(fs) if x[0] == k), -1)
                if f < 0:
                    ny_jskip(p)
                else:
                    p.path.append(k)
                    vals[f] = ny_jdec(p, fs[f][2])
                    p.path.pop()
                    seen[f] = True
                if not ny_jnext(p, "}"):
                    break
        if not all(seen):
            missing = fs[seen.index(False)][0]
            ny_panic("E0345", f"json.parse: missing field \"{missing}\" at {ny_jpath(p)}",
                     "the JSON object must have every field of the struct", p.line, p.col)
        return d(*vals)
    if isinstance(d, tuple):
        out = NyList()
        if ny_jopen(p, "[", "an array"):
            while True:
                p.path.append(len(out))
                out.append(ny_jdec(p, d[1]))
                p.path.pop()
                if not ny_jnext(p, "]"):
                    break
        return out
    if d == "i":
        if c != "-" and not "0" <= c <= "9":
            ny_jtype(p, "an int")
        t, whole = ny_jnumber(p)
        if whole and len(t.lstrip("-")) <= 19:
            v = int(t)
            if -9223372036854775808 <= v <= 9223372036854775807:
                return v
        ny_jtype(p, "an int")
    if d == "f":
        if c != "-" and not "0" <= c <= "9":
            ny_jtype(p, "a number")
        return float(ny_jnumber(p)[0])
    if d == "b":
        if c not in "tf":
            ny_jtype(p, "true or false")
        return ny_jliteral(p) == "true"
    if d == "c":
        if c != '"':
            ny_jtype(p, "a one-character string")
        s = ny_jstring(p)
        if len(s) != 1:
            ny_jtype(p, "a one-character string")
        return s
    if c != '"':
        ny_jtype(p, "a string")
    return ny_jstring(p)


def ny_jparse(text, d, line, col):
    p = NyJP(text, line, col)
    v = ny_jdec(p, d)
    ny_jws(p)
    if p.i < len(text):
        ny_jsyntax(p, "text after the value")
    return v
