# ---- Nyra runtime (Python) ----------------------------------------------------------------
# Ints wrap at 64 bits (ny_i64), floats print like JavaScript's String(x), lengths and indexes
# count characters, a char is a one-character str, and arrays and structs are values: they are
# copied on write, through a mark on the ones that have more than one owner (ny_shared).
# (The imports are at the top of the file.)

NY_FILE = @FILE@


class NyPanic(Exception):
    """A Nyra runtime error: the entry point prints it and exits with 101."""


def ny_panic(code, msg, hint, line, col):
    if os.environ.get("NYRA_JSON"):
        err = {"code": code, "message": msg, "file": NY_FILE, "line": line, "col": col, "hint": hint, "runtime": True}
        text = json.dumps({"ok": False, "errors": [err]}, ensure_ascii=False, separators=(",", ":"))
    else:
        text = f"runtime error[{code}]: {msg}\n  --> {NY_FILE}:{line}:{col}\n  = hint: {hint}\n  = explain: nyra explain {code}"
    raise NyPanic(text)


def ny_oom(line, col):
    ny_panic("E0249", "out of memory", "the program needs more memory than the system gave it", line, col)


def ny_main(main):
    """Runs `main`; a runtime error goes to stderr with exit code 101, after all earlier output."""
    sys.stdout.reconfigure(encoding="utf-8", newline="\n")
    sys.stderr.reconfigure(encoding="utf-8", newline="\n")
    sys.setrecursionlimit(100000)
    try:
        try:
            main()
        except MemoryError:
            ny_oom(0, 0)
    except NyPanic as e:
        sys.stdout.flush()
        print(e, file=sys.stderr)
        sys.exit(101)


# ---- ints: 64 bits, wrapping on overflow ----

def ny_i64(x):
    """`x` wrapped to a 64-bit int, like int overflow on every backend."""
    if -9223372036854775808 <= x <= 9223372036854775807:
        return x
    return ((x + 9223372036854775808) & 0xFFFFFFFFFFFFFFFF) - 9223372036854775808


def ny_quot(a, b):
    """int `/`: truncates toward zero (Python's `//` rounds down)."""
    q = abs(a) // abs(b)
    return ny_i64(q if (a < 0) == (b < 0) else -q)


def ny_rem(a, b):
    """int `%`: the sign of the dividend (Python's `%` takes the divisor's)."""
    r = abs(a) % abs(b)
    return -r if a < 0 else r


def ny_div(a, b, line, col):
    if b == 0:
        ny_panic("E0241", "division by zero", "check the divisor first", line, col)
    return ny_quot(a, b)


def ny_mod(a, b, line, col):
    if b == 0:
        ny_panic("E0241", "division by zero", "check the divisor first", line, col)
    return ny_rem(a, b)


def ny_check_step(k, line, col):
    """`for i in a..b step k`: a step of 0 would never end."""
    if k == 0:
        ny_panic("E0243", "range step must not be 0",
                 "use a positive step to count up and a negative one to count down", line, col)


def ny_f2i(x, line, col):
    """int(x) of a float: truncates toward zero; NaN or a value outside the int range is an error."""
    if x != x or x >= 9223372036854775807.0 or x < -9223372036854775808.0:
        ny_panic("E0245", f"cannot convert {ny_num(x)} to int",
                 "int(x) needs a float that is not NaN and fits in an int", line, col)
    return int(x)


# ---- floats ----

def ny_fdiv(a, b):
    """float `/`: dividing by zero gives an infinity or NaN, as in IEEE 754."""
    if b != 0:
        return a / b
    if a != a or a == 0:
        return math.nan
    return math.copysign(math.inf, a) * math.copysign(1.0, b)


def ny_num(x):
    """A float as JavaScript's String(x) shows it: the shortest digits that read back the same."""
    if x != x:
        return "NaN"
    if x == 0:
        return "0"
    if x == math.inf:
        return "Infinity"
    if x == -math.inf:
        return "-Infinity"
    r = repr(x)
    sign = ""
    if r[0] == "-":
        sign, r = "-", r[1:]
    mant, _, exp = r.partition("e")
    whole, _, frac = mant.partition(".")
    digits = whole + frac
    n = len(whole) + (int(exp) if exp else 0)  # the value is 0.DIGITS * 10^n
    stripped = digits.lstrip("0")
    n -= len(digits) - len(stripped)
    digits = stripped.rstrip("0")
    k = len(digits)
    if k <= n <= 21:
        return sign + digits + "0" * (n - k)
    if 0 < n <= 21:
        return sign + digits[:n] + "." + digits[n:]
    if -6 < n <= 0:
        return sign + "0." + "0" * -n + digits
    e = n - 1
    m = digits[0] + ("." + digits[1:] if k > 1 else "")
    return sign + m + "e" + ("+" if e > 0 else "-") + str(abs(e))


def ny_bool(b):
    return "true" if b else "false"


# ---- strings: Python strs count characters (code points), like Nyra ----

NY_UPPER = str.maketrans("abcdefghijklmnopqrstuvwxyz", "ABCDEFGHIJKLMNOPQRSTUVWXYZ")
NY_LOWER = str.maketrans("ABCDEFGHIJKLMNOPQRSTUVWXYZ", "abcdefghijklmnopqrstuvwxyz")


def ny_oob(i, n, line, col):
    ny_panic("E0240", f"index {i} is out of bounds for length {n}",
             "valid indexes are 0 to len - 1; compare with `.len()` first", line, col)


def ny_range(a, b, n, line, col):
    if a < 0 or a > b or b > n:
        ny_panic("E0240", f"range {a}..{b} is out of bounds for length {n}",
                 "a range a..b needs 0 <= a <= b <= len", line, col)


def ny_char_at(s, i, line, col):
    if i < 0 or i >= len(s):
        ny_oob(i, len(s), line, col)
    return s[i]


def ny_str_slice(s, a, b, line, col):
    ny_range(a, b, len(s), line, col)
    return s[a:b]


def ny_replace(s, old, new, line, col):
    if old == "":
        ny_panic("E0243", "replace() needs a non-empty pattern", "the text to replace can't be \"\"", line, col)
    return s.replace(old, new)


def ny_trim(s):
    return s.strip(" \t\n\r")


def ny_upper(s):
    """ASCII letters only, like every backend."""
    return s.translate(NY_UPPER)


def ny_lower(s):
    return s.translate(NY_LOWER)


def ny_utf8_len(s):
    return len(s) if s.isascii() else len(s.encode("utf-8"))


def ny_str_repeat(s, n, line, col):
    if n < 0:
        ny_panic("E0243", f"repeat count must be >= 0, got {n}", "repeat(n) needs n >= 0", line, col)
    # the longest text every backend can make (in UTF-8 bytes)
    if ny_utf8_len(s) * n > 536870888:
        ny_oom(line, col)
    return s * n


def ny_pad(s, n, c, left):
    """`s.pad_left(n, c)` / `s.pad_right(n, c)`: `c` added until `s` has `n` characters."""
    missing = n - len(s)
    if missing <= 0:
        return s
    if missing > 536870888:
        ny_oom(0, 0)
    return c * missing + s if left else s + c * missing


# char tests: ASCII only, like upper() and lower()
def ny_is_digit(c):
    return "0" <= c <= "9"


def ny_is_upper(c):
    return "A" <= c <= "Z"


def ny_is_lower(c):
    return "a" <= c <= "z"


def ny_is_letter(c):
    return "A" <= c <= "Z" or "a" <= c <= "z"


def ny_is_space(c):
    return c in " \t\n\r"


def ny_char_upper(c):
    return chr(ord(c) - 32) if "a" <= c <= "z" else c


def ny_char_lower(c):
    return chr(ord(c) + 32) if "A" <= c <= "Z" else c


def ny_chr(n, line, col):
    if n < 0 or n > 1114111 or 55296 <= n <= 57343:
        ny_panic("E0246", f"char({n}): not a valid character code",
                 "character codes go from 0 to 1114111, except 55296 to 57343", line, col)
    return chr(n)


def ny_shown(s):
    """The text of a string in an error message: control characters as escapes."""
    out = []
    for c in s:
        if c == "\n":
            out.append("\\n")
        elif c == "\t":
            out.append("\\t")
        elif c == "\r":
            out.append("\\r")
        elif ord(c) < 0x20:
            out.append(f"\\u{ord(c):04x}")
        else:
            out.append(c)
    return "".join(out)


NY_INT = re.compile(r"-?[0-9]+")
NY_FLOAT = re.compile(r"-?[0-9]+(\.[0-9]+)?([eE][+-]?[0-9]+)?")


def ny_int(s, line, col):
    """int(s): digits with an optional `-` that fit in an int, nothing else."""
    if NY_INT.fullmatch(s):
        v = int(s)
        if -9223372036854775808 <= v <= 9223372036854775807:
            return v
    ny_panic("E0244", f"cannot parse \"{ny_shown(s)}\" as int",
             "int(s) accepts only digits with an optional `-`, e.g. \"-42\"", line, col)


def ny_float(s, line, col):
    if not NY_FLOAT.fullmatch(s):
        ny_panic("E0244", f"cannot parse \"{ny_shown(s)}\" as float",
                 "float(s) accepts digits with an optional `-`, `.` part and exponent, e.g. \"-1.5e3\"", line, col)
    return float(s)


def ny_split(s, sep, line, col):
    if sep == "":
        ny_panic("E0243", "split() needs a non-empty separator", "for the characters of a string use `s.chars()`", line, col)
    return NyList(s.split(sep))


# ---- arrays and structs: values, copied on write ----

class NyList(list):
    """A Nyra array: a list that is copied before a write when it is shared."""
    ny_shared = False


class NyDict(dict):
    """A Nyra map: a dict (insertion order) that is copied before a write when it is shared."""
    ny_shared = False


def ny_mget(m, k, kt, line, col):
    """`m[k]`: E0247 when the key is missing (`kt` is the key's type, for the message)."""
    if k not in m:
        ny_panic("E0247", f"key {ny_show(k, kt)} is not in the map", "check with `m.has(k)` first, or read it with `m.get(k, default)`", line, col)
    return m[k]


def ny_share(v):
    """`v` gets one more owner: a write to any of them copies it first."""
    v.ny_shared = True
    return v


def ny_share_all(xs):
    """Marks the elements of a new array shared (another array has them too) and returns it."""
    if xs and hasattr(xs[0], "ny_shared"):
        for v in xs:
            v.ny_shared = True
    return xs


def ny_copy(v):
    """A copy of one level: the copy is not shared, the values it now shares are."""
    if isinstance(v, NyList):
        return ny_share_all(NyList(v))
    if isinstance(v, NyDict):
        c = NyDict(v)
        ny_share_all(list(c.values()))
        return c
    return v.ny_copy()


def ny_unique(v):
    """`v` itself when it has one owner, else a copy: what a write needs."""
    return ny_copy(v) if v.ny_shared else v


def ny_unique_item(xs, i):
    """`xs[i]` made unique for a write (copied and stored back when it was shared)."""
    v = xs[i]
    if v.ny_shared:
        v = xs[i] = ny_copy(v)
    return v


def ny_unique_attr(obj, name):
    """The field `name` of `obj` made unique for a write."""
    v = getattr(obj, name)
    if v.ny_shared:
        v = ny_copy(v)
        setattr(obj, name, v)
    return v


def ny_ck(xs, i, line, col):
    """An index that must be in bounds (E0240)."""
    if i < 0 or i >= len(xs):
        ny_oob(i, len(xs), line, col)
    return i


def ny_at(xs, i, line, col):
    """xs[i], checked (E0240)."""
    if i < 0 or i >= len(xs):
        ny_oob(i, len(xs), line, col)
    return xs[i]


def ny_pop(xs, line, col):
    if not xs:
        ny_panic("E0242", "pop() on an empty array", "check `xs.len() > 0` first", line, col)
    return xs.pop()


def ny_insert(xs, i, v, line, col):
    if i < 0 or i > len(xs):
        ny_panic("E0240", f"index {i} is out of bounds for length {len(xs)}", "insert(i, x) needs 0 <= i <= len", line, col)
    xs.insert(i, v)


def ny_remove(xs, i, line, col):
    ny_ck(xs, i, line, col)
    return xs.pop(i)


def ny_swap(xs, i, j, line, col):
    ny_ck(xs, i, line, col)
    ny_ck(xs, j, line, col)
    xs[i], xs[j] = xs[j], xs[i]


def ny_slice(xs, a, b, line, col):
    ny_range(a, b, len(xs), line, col)
    return ny_share_all(NyList(xs[a:b]))


def ny_concat(a, b):
    return ny_share_all(NyList(a + b))


def ny_repeat(xs, n, line, col):
    if n < 0:
        ny_panic("E0243", f"repeat count must be >= 0, got {n}", "repeat(n) needs n >= 0", line, col)
    # the longest array every backend makes with repeat
    if len(xs) * n > 100000000:
        ny_oom(line, col)
    return ny_share_all(NyList(xs * n))


def ny_extend(xs, ys):
    """`xs += ys` on a unique `xs` (`xs += xs` doubles it)."""
    xs.extend(ny_share_all(list(ys)))


def ny_eq(a, b):
    """Deep equality without Python's shortcut for the same object, so NaN never equals itself."""
    if isinstance(a, NyList):
        return len(a) == len(b) and all(ny_eq(x, y) for x, y in zip(a, b))
    if isinstance(a, NyDict):
        return len(a) == len(b) and all(k in b and ny_eq(v, b[k]) for k, v in a.items())
    return a == b


def ny_index_of(xs, v):
    for i, x in enumerate(xs):
        if ny_eq(x, v):
            return i
    return -1


def ny_float_key(x):
    """Sorts floats like every backend: NaN after every number, equal values keep their order."""
    return (x != x, x)


# ---- printing: arrays and structs as Nyra code ----
# `t` is the type: "i" int, "f" float, "b" bool, "c" char, "s" str, "[" + the element type, "S" struct.

NY_ESC = {"\\": "\\\\", "\n": "\\n", "\t": "\\t", "\r": "\\r"}


def ny_quoted(text, quote):
    return quote + "".join("\\" + c if c == quote else NY_ESC.get(c, c) for c in text) + quote


def ny_show(v, t):
    k = t[0]
    if k == "i":
        return str(v)
    if k == "f":
        return ny_num(v)
    if k == "b":
        return ny_bool(v)
    if k == "c":
        return ny_quoted(v, "'")
    if k == "s":
        return ny_quoted(v, '"')
    if k == "[":
        e = t[1:]
        return "[" + ", ".join(ny_show(x, e) for x in v) + "]"
    if k == "{":
        # a map: "{" + the key type (one letter) + the value type
        if not v:
            return "[:]"
        kt, vt = t[1], t[2:]
        return "[" + ", ".join(ny_show(a, kt) + ": " + ny_show(b, vt) for a, b in v.items()) + "]"
    return repr(v)
