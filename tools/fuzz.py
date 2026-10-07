#!/usr/bin/env python3
"""A differential fuzzer for Nyra's two backends: C (native, through gcc) and JavaScript (Node).

Each seed deterministically generates one random, well-typed Nyra program. The program is built
once per backend and run: natively with NYRA_LEAKCHECK=1 (exit 102 is a memory bug) and on Node.
Both must print the same stdout and exit with the same code; for a runtime error (exit 101) the
first stderr line and the position must match too. A fraction of the programs is built again with
NYRA_OPT=0, which must not change anything. Failing programs are shrunk and written to
tools/fuzz_failures/<kind>-<seed>.nyra.

    python tools/fuzz.py --seed 1 --count 500 --jobs 10    # run seeds 1..500
    python tools/fuzz.py --print 42                         # show the program of seed 42
    python tools/fuzz.py --check prog.nyra                  # run one file through the oracle
    python tools/fuzz.py --minimize prog.nyra               # shrink a failing file

The generator keeps an abstract state for every variable (how long its arrays and strings can
be, at every level), so indexes stay in bounds, `pop` never sees an empty array, sizes stay
small and every loop is bounded. It never relies on that state being exact: a wrong guess only
makes a program end early with the same runtime error on both backends.
"""

import argparse
import hashlib
import os
import random
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from concurrent.futures import ThreadPoolExecutor

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EXE = ".exe" if os.name == "nt" else ""
NYRA = os.path.join(ROOT, "target", "release", "nyra" + EXE)
FAILURES = os.path.join(ROOT, "tools", "fuzz_failures")

# ---------------------------------------------------------------------------------------------
# Types and shapes
# ---------------------------------------------------------------------------------------------

INT, FLOAT, BOOL, STR, CHAR = "int", "float", "bool", "str", "char"
ACAP = 6         # the most elements a stored array may hold
SCAP = 40        # the most characters a stored string may hold
DMAX = 3         # how many arrays deep a recursive struct may nest
VB = 2 ** 31     # int variables are assumed to stay within +-VB
SAFE = 2 ** 50   # no int expression may exceed this (JS numbers are exact below 2^53)
WRAP = 1000003   # a stored int that could be larger is taken modulo this


def arr(t):
    return ("arr", t)


def is_arr(t):
    return isinstance(t, tuple) and t[0] == "arr"


def is_struct(t):
    return isinstance(t, tuple) and t[0] == "struct"


def mag(iv):
    return max(abs(iv[0]), abs(iv[1]))


# A shape is what the generator knows about a value that has a length:
#   ("s", lo, hi)            a string of lo..hi characters
#   ("a", lo, hi, el)        an array of lo..hi elements, each of shape el
#   ("t", ((field, sh), ..)) a struct
#   None                     a scalar (int, float, bool, char)
#   "E"                      the element shape of an array that is always empty

def join(a, b):
    """The shape of a value that is either a or b."""
    if a is None or b is None:
        return None
    if a == "E":
        return b
    if b == "E":
        return a
    if a[0] == "s":
        return ("s", min(a[1], b[1]), max(a[2], b[2]))
    if a[0] == "a":
        if a[2] == 0:
            el = b[3]
        elif b[2] == 0:
            el = a[3]
        else:
            el = join(a[3], b[3])
        return ("a", min(a[1], b[1]), max(a[2], b[2]), el)
    return ("t", tuple((f, join(x, y)) for (f, x), (_, y) in zip(a[1], b[1])))


def fits(s, env):
    """True if every value of shape s is allowed by the envelope env (lower and upper bounds)."""
    if env is None or s is None or s == "E":
        return True
    if env == "E":
        return False
    if s[0] == "s":
        return s[1] >= env[1] and s[2] <= env[2]
    if s[0] == "a":
        if s[1] < env[1] or s[2] > env[2]:
            return False
        return s[2] == 0 or fits(s[3], env[3])
    return all(fits(x, y) for (_, x), (_, y) in zip(s[1], env[1]))


def lo_of(sh):
    return sh[1] if isinstance(sh, tuple) and sh[0] in "sa" else 0


def hi_of(sh):
    return sh[2] if isinstance(sh, tuple) and sh[0] in "sa" else 0


def with_lo(sh, lo):
    return (sh[0], lo, max(lo, sh[2])) + sh[3:]


def with_hi(sh, hi):
    if sh[0] == "s":
        return ("s", min(sh[1], hi), hi)
    return ("a", min(sh[1], hi), hi, sh[3] if hi > 0 else "E")


def grow_shape(s, add):
    """The shape of s with the elements (or characters) of add appended."""
    if s[0] == "s":
        return ("s", s[1] + add[1], s[2] + add[2])
    el = add[3] if s[2] == 0 else (s[3] if add[2] == 0 else join(s[3], add[3]))
    return ("a", s[1] + add[1], s[2] + add[2], el)


def shrink_shape(s, n=1):
    """The shape of the array s after n elements were removed."""
    hi = max(0, s[2] - n)
    return ("a", max(0, s[1] - n), hi, s[3] if hi > 0 else "E")


# ---------------------------------------------------------------------------------------------
# Literal pools
# ---------------------------------------------------------------------------------------------

STR_POOL = [
    "", "a", "ab", "abc", "hello", "Hello World", " padded ", "a,b,,c", "x-y-z", "one two three",
    "é", "naïve", "héllo wörld", "ß", "Ωmega", "日本語", "😀", "a😀b", "😀🎉", "é", "ｚ", "",
    "tab\there", "line\nbreak", "quote\"d", "back\\slash", "{}", "{x}", "CamelCase", "UPPER", "MiXeD",
    "123", "-42", "007", "0", "3.5", "-0.25", "1e3", " 12", "12 ", "NaN", "aaa", "abab", "  ", "\r\n",
]
SEP_POOL = [",", "-", " ", "a", "ab", "é", "😀", ", ", "ll", "\n", "--", "aa"]
NUM_STRS = ["0", "7", "-7", "42", "007", "-0", "123456", "999999999999", "-2147483648", "65536"]
FLOAT_STRS = ["0.5", "-2.25", "1e3", "2.5e-3", "1E21", "-0", "0.1", "123.456", "1e999", "-1e-999",
              "5e-324", "1.7976931348623157e308", "9007199254740993", "0.30000000000000004", "4.35",
              "1e-7", "123456789012345678901234567890", "0.000001", "2.2250738585072014e-308"]
CHAR_POOL = ["a", "z", "A", "Z", "0", "9", " ", "\n", "\t", "'", "\\", '"', "é", "ß", "😀", "中", "{", "}",
             ",", "-", "ｚ", ""]
FLOAT_POOL = ["0.0", "1.0", "1.5", "2.5", "0.1", "0.2", "0.3", "3.14159", "100.0", "0.5", "7.25",
              "1000000000000000000000.0", "0.000001", "0.0000001", "123456789.125", "9007199254740993.0",
              "0.30000000000000004", "1.7976931348623157", "4.9", "255.0", "0.0625", "4.35", "1.005"]
SPECIAL_FLOATS = [("(0.0 / 0.0)", True), ("(1.0 / 0.0)", True), ("(-1.0 / 0.0)", True), ("-0.0", False)]
# Float literals that overflow to infinity or are subnormal
EXTREME_FLOATS = ["1" + "0" * 400 + ".0", "0." + "0" * 323 + "5", "0." + "0" * 307 + "22250738585072014",
                  "179769313486231570" + "0" * 291 + ".0"]

VAR_BASES = ["a", "b", "c", "n", "m", "k", "s", "t", "u", "x", "y", "z", "xs", "ys", "zs", "g", "row",
             "acc", "tmp", "val", "item", "p", "q", "w", "res", "word", "names", "nums", "grid", "flag",
             "ch", "cs", "h", "r", "v", "d", "e", "o"]
FN_BASES = ["f", "calc", "make", "step", "fold", "pick", "grow", "mix", "build", "walk"]
STRUCT_NAMES = ["Pt", "Item", "User", "Tag", "Box", "Node", "Pair", "Cell", "Rec", "Doc"]
FIELD_NAMES = ["x", "y", "n", "id", "name", "tags", "vals", "kids", "w", "h", "label", "items", "grid",
               "flag", "ch", "ratio", "count", "text", "parts", "next"]
# Names that are fine in Nyra but are reserved, predefined or special in C or JavaScript
EXOTIC_NAMES = [
    "WIN32", "_WIN32", "unix", "INT32_MAX", "SIZE_MAX", "BUFSIZ", "EXIT_SUCCESS", "RAND_MAX", "DBL_MIN",
    "FLT_MAX", "SEEK_SET", "FILE", "int32_t", "uint8_t", "wchar_t", "abs", "fflush", "memset", "rand",
    "__LINE__", "__func__", "asm", "typeof", "_Bool", "I", "Map", "Set", "Date", "Reflect", "isNaN",
    "parseInt", "Boolean", "Function", "async", "of", "get", "set", "ny", "nyx", "NYL", "nyS_Pt",
    "ny_s", "nyrt_t1", "b1", "it", "length", "toString", "valueOf", "é", "naïve", "x²", "ǅ", "ª", "日本",
]
EXOTIC_FIELDS = ["length", "toString", "valueOf", "constructor", "__proto__", "prototype", "data", "len",
                 "rc", "cap", "ty", "v", "ny_s", "ny_cp", "hasOwnProperty", "then", "double", "int",
                 "char", "float", "unsigned", "register", "default", "class", "function", "this", "é"]


def str_lit(s):
    """A Nyra string literal for the text s."""
    esc = {"\\": "\\\\", '"': '\\"', "\n": "\\n", "\t": "\\t", "\r": "\\r", "{": "{{", "}": "}}"}
    return '"' + "".join(esc.get(c, c) for c in s) + '"'


def char_lit(c):
    esc = {"\\": "\\\\", "'": "\\'", "\n": "\\n", "\t": "\\t", "\r": "\\r"}
    return "'" + esc.get(c, c) + "'"


# ---------------------------------------------------------------------------------------------
# The generator
# ---------------------------------------------------------------------------------------------

class E:
    """A generated expression: its code and what is known about its value."""
    __slots__ = ("code", "ty", "sh", "iv", "atom", "fv")

    def __init__(self, code, ty, sh=None, iv=None, atom=True, fv=None):
        self.code = code
        self.ty = ty
        self.sh = sh      # shape (strings, arrays, structs)
        self.iv = iv      # int interval
        self.atom = atom  # usable as an operand or a receiver without parentheses
        self.fv = fv      # float: a bound on the magnitude when it is surely finite


def par(e):
    return e.code if e.atom else "(" + e.code + ")"


class Var:
    __slots__ = ("name", "ty", "kind", "sh", "env", "freed", "arena", "frozen", "iv", "ld")

    def __init__(self, name, ty, kind, sh, env, arena, ld, iv=None):
        self.name = name
        self.ty = ty
        self.kind = kind     # let var param inout loop counter
        self.sh = sh         # current shape
        self.env = env       # envelope: the value always stays within it (floors and caps)
        self.freed = False
        self.arena = arena   # arena depth where it was declared
        self.frozen = False  # may not change here (an outer variable of a loop or an arena)
        self.iv = iv         # loop variables and counters: their interval
        self.ld = ld         # loop depth where it was declared

    def copy(self):
        v = Var.__new__(Var)
        for k in Var.__slots__:
            setattr(v, k, getattr(self, k))
        return v

    def can_change(self):
        return self.kind in ("var", "inout") and not self.frozen


class Param:
    __slots__ = ("name", "ty", "inout", "env")

    def __init__(self, name, ty, inout, env):
        self.name, self.ty, self.inout, self.env = name, ty, inout, env


class Fn:
    __slots__ = ("name", "params", "ret", "ret_env", "index")

    def __init__(self, name, params, ret, ret_env, index):
        self.name, self.params, self.ret, self.ret_env, self.index = name, params, ret, ret_env, index


FAIL = object()   # an update that would break a variable's envelope


class Place:
    """A changeable place: a variable, then fields and elements (`g[i].tags`)."""
    __slots__ = ("root", "steps", "code", "ty", "sh", "env")

    def __init__(self, root, steps, code, ty, sh, env):
        self.root, self.steps, self.code, self.ty, self.sh, self.env = root, steps, code, ty, sh, env


FEATURES = {
    # name: on by default
    "exotic_names": False,     # identifiers that are special in C or JavaScript
    "extreme_floats": False,   # float literals that overflow to infinity or are subnormal
    "nul_char": False,         # char(0)
    "risky": True,             # rarely: indexes, divisors and parses that may fail at run time
    "free": True,
    "keep": True,
    "arena": True,
    "inout": True,
    "recursive_structs": True,
    "name_reuse": True,        # a block reuses the name of a variable of a closed sibling block
    "evalorder": True,         # statements whose operands change the array they read
    "pod_structs": True,       # structs without a string or an array inside
    "struct_field_reads": True,  # a struct read out of a field (`p.inner`) as a value
}


class Gen:
    def __init__(self, seed, feats=None):
        self.r = random.Random(seed)
        self.f = dict(FEATURES)
        if feats:
            self.f.update(feats)
        self.structs = {}       # name -> [(field, type)]
        self.fns = []
        self.vars = {}
        self.scopes = []
        self.dead = []          # names declared in closed blocks: may be declared again
        self.counter = 0
        self.used_names = set()
        self.cur = None
        self.loop_depth = 0
        self.arena_depth = 0
        self.budget = 0
        self.nomut_roots = ()   # variables the expression being generated must not change
        self.nmut = 0           # how many changes expressions have made (pop, inout)

    # ---- helpers -----------------------------------------------------------------------------

    def chance(self, p):
        return self.r.random() < p

    def pick(self, xs):
        return xs[self.r.randrange(len(xs))]

    def wpick(self, pairs):
        pairs = [(w, v) for w, v in pairs if w > 0]
        x = self.r.uniform(0, sum(w for w, _ in pairs))
        for w, v in pairs:
            x -= w
            if x <= 0:
                return v
        return pairs[-1][1]

    def tname(self, t):
        if is_arr(t):
            return "[" + self.tname(t[1]) + "]"
        if is_struct(t):
            return t[1]
        return t

    def managed(self, t, seen=()):
        if t == STR or is_arr(t):
            return True
        if is_struct(t):
            if t[1] in seen:
                return False
            return any(self.managed(ft, seen + (t[1],)) for _, ft in self.structs[t[1]])
        return False

    def has_shape(self, t):
        return t == STR or is_arr(t) or is_struct(t)

    def top(self, t, d=DMAX):
        """The widest shape of a stored value of type t."""
        if t == STR:
            return ("s", 0, SCAP)
        if is_arr(t):
            if d <= 0:
                return ("a", 0, 0, "E")
            return ("a", 0, ACAP, self.top(t[1], d - 1))
        if is_struct(t):
            return ("t", tuple((f, self.top(ft, d)) for f, ft in self.structs[t[1]]))
        return None

    def repr_len(self, t, sh, inner=False):
        """An upper bound on the length of str(x)."""
        if t == INT:
            return 20
        if t == FLOAT:
            return 24
        if t == BOOL:
            return 5
        if t == CHAR:
            return 4 if inner else 1
        if not isinstance(sh, tuple):
            sh = self.top(t)
        if t == STR:
            return 2 + 2 * sh[2] if inner else sh[2]
        if is_arr(t):
            if sh[2] == 0:
                return 2
            return 2 + sh[2] * (self.repr_len(t[1], sh[3], True) + 2)
        fields = self.structs[t[1]]
        return len(t[1]) + 2 + sum(len(f) + 4 + self.repr_len(ft, fs, True)
                                   for (f, ft), (_, fs) in zip(fields, sh[1]))

    def save(self):
        return ({n: v.copy() for n, v in self.vars.items()}, [list(s) for s in self.scopes], list(self.dead),
                self.nmut)

    def restore(self, saved):
        vs, scopes, dead, nmut = saved
        self.vars = {n: v.copy() for n, v in vs.items()}
        self.scopes = [list(s) for s in scopes]
        self.dead = list(dead)
        self.nmut = nmut

    def vstate(self):
        return {n: v.copy() for n, v in self.vars.items()}

    def set_vstate(self, st):
        self.vars = {n: v.copy() for n, v in st.items()}

    def join_states(self, a, b):
        out = {}
        for n, va in a.items():
            vb = b.get(n)
            v = va.copy()
            if vb is not None:
                v.sh = join(va.sh, vb.sh)
                v.freed = va.freed or vb.freed
            out[n] = v
        return out

    # ---- names -------------------------------------------------------------------------------

    def fresh(self, base):
        self.counter += 1
        return f"{base}{self.counter}"

    def new_var_name(self):
        if self.f["exotic_names"] and self.chance(0.06):
            for _ in range(5):
                n = self.pick(EXOTIC_NAMES)
                if n not in self.used_names and n not in self.structs:
                    self.used_names.add(n)
                    return n
        if self.f["name_reuse"] and self.dead and self.chance(0.25):
            n = self.pick(self.dead)
            self.dead.remove(n)
            return n
        return self.fresh(self.pick(VAR_BASES))

    def declare(self, name, t, kind, sh, env=None, iv=None):
        if env is None:
            env = self.top(t)
        v = Var(name, t, kind, sh, env, self.arena_depth, self.loop_depth, iv)
        self.vars[name] = v
        self.scopes[-1].append(name)
        if name in self.dead:
            self.dead.remove(name)
        return v

    def live(self, pred=None):
        return [v for v in self.vars.values() if not v.freed and (pred is None or pred(v))]

    # ---- types -------------------------------------------------------------------------------

    def rand_type(self, depth=0):
        opts = [(14, INT), (4, FLOAT), (3, BOOL), (12, STR), (4, CHAR)]
        if depth < 2:
            opts.append((22 if depth == 0 else 8, "arr"))
        if self.structs:
            opts.append((12 if depth == 0 else 6, "struct"))
        k = self.wpick(opts)
        if k == "arr":
            return arr(self.rand_type(depth + 1))
        if k == "struct":
            return ("struct", self.pick(sorted(self.structs)))
        return k

    def gen_structs(self):
        n = self.wpick([(2, 0), (3, 1), (4, 2), (2, 3)])
        names = self.r.sample(STRUCT_NAMES, n)
        lines = []
        for name in names:
            nf = self.wpick([(1, 0), (4, 1), (6, 2), (5, 3), (3, 4)])
            fields = []
            used = set()
            for _ in range(nf):
                pool = EXOTIC_FIELDS if self.f["exotic_names"] and self.chance(0.15) else FIELD_NAMES
                fname = self.pick(pool)
                if fname in used:
                    continue
                used.add(fname)
                earlier = list(self.structs)
                opts = [(5, INT), (2, FLOAT), (2, BOOL), (5, STR), (2, CHAR), (4, arr(INT)), (3, arr(STR)),
                        (1, arr(FLOAT)), (1, arr(CHAR)), (2, arr(arr(INT)))]
                if earlier:
                    opts.append((3, ("struct", self.pick(earlier))))
                    opts.append((3, arr(("struct", self.pick(earlier)))))
                if self.f["recursive_structs"]:
                    opts.append((2, arr(("struct", name))))
                fields.append((fname, self.wpick(opts)))
            if not self.f["pod_structs"] and not any(self.managed(ft) or ft == arr(("struct", name))
                                                     for _, ft in fields):
                fields.append(("label" if "label" not in used else "text_", STR))
            self.structs[name] = fields
            self.used_names.add(name)
            body = [f"{f}: {self.tname(t)}" for f, t in fields]
            if body and self.chance(0.4):
                lines.append(f"struct {name} {{")
                lines += ["    " + b for b in body]
                lines.append("}")
            else:
                lines.append(f"struct {name} {{ {', '.join(body)} }}" if body else f"struct {name} {{}}")
        return lines

    # ---- ints --------------------------------------------------------------------------------

    def small_int(self, e, m):
        """e, or e % m when e could be larger than m."""
        if mag(e.iv) <= m:
            return e
        lo = 0 if e.iv[0] >= 0 else -(m - 1)
        hi = 0 if e.iv[1] <= 0 else m - 1
        return E(f"{par(e)} % {m}", INT, iv=(lo, hi), atom=False)

    def wrap_int(self, e):
        """An int that is stored must stay within +-VB."""
        return self.small_int(e, VB) if mag(e.iv) > VB else e

    def nonzero(self, e):
        """A divisor that is never zero."""
        if e.iv[0] > 0 or e.iv[1] < 0:
            return e
        return E(f"{par(e)} % 7 + 8", INT, iv=(2, 14), atom=False)

    # ---- indexes and places ------------------------------------------------------------------

    def index(self, lo, prefix, q):
        """A pure int expression that is a valid index of an array (or string) that has at least
        lo elements. prefix: pure code of that array, for `xs[xs.len() - 1]`."""
        loops = [v for v in self.live(lambda v: v.kind in ("loop", "counter") and v.ty == INT and v.iv)
                 if v.iv[0] >= 0 and v.iv[1] <= lo - 1]
        k = self.wpick([(6, "lit"), (3 if prefix else 0, "len"), (4 if loops else 0, "loop"), (3, "mod")])
        if k == "lit":
            return str(self.r.randrange(lo))
        if k == "len":
            return f"{prefix}.len() - {self.r.randint(1, lo)}"
        if k == "loop":
            return self.pick(loops).name
        e = self.expr(INT, None, 1, q=q, mut=False)
        if e.iv[0] >= 0:
            return f"{par(e)} % {lo}"
        return f"({par(e)} % {lo} + {lo}) % {lo}"

    def place_paths(self, root, want, max_steps=3, need_steps=False):
        """Abstract paths from root to a part whose type satisfies want: lists of steps
        ("i", lo) or ("f", field, k). Only arrays known to be non-empty are indexed."""
        out = []

        def walk(t, sh, steps):
            if want(t) and (steps or not need_steps):
                out.append(list(steps))
            if len(steps) >= max_steps or not isinstance(sh, tuple):
                return
            if is_arr(t) and sh[1] >= 1:
                walk(t[1], sh[3], steps + [("i", sh[1])])
            elif is_struct(t):
                for k, (f, ft) in enumerate(self.structs[t[1]]):
                    walk(ft, sh[1][k][1], steps + [("f", f, k)])

        walk(root.ty, root.sh, [])
        return out

    def build_place(self, root, steps, q=True):
        code = root.name
        t, sh, env = root.ty, root.sh, root.env
        built = []
        for s in steps:
            if s[0] == "i":
                code = f"{code}[{self.index(sh[1], code, q)}]"
                t, sh, env = t[1], sh[3], env[3]
                built.append(("i",))
            else:
                _, f, k = s
                code = f"{code}.{f}"
                t, sh, env = self.structs[t[1]][k][1], sh[1][k][1], env[1][k][1]
                built.append(("f", f, k))
        return Place(root, built, code, t, sh, env)

    def update_place(self, place, fn):
        """Applies fn to the shape at place (the element that changes gets the new shape, the
        others keep theirs) and returns the new shape of the root variable, or FAIL if that
        breaks the variable's envelope."""
        def upd(sh, steps):
            if not steps:
                return fn(sh)
            s = steps[0]
            if s[0] == "i":
                return ("a", sh[1], sh[2], join(sh[3], upd(sh[3], steps[1:])))
            k = s[2]
            fields = list(sh[1])
            fields[k] = (fields[k][0], upd(sh[1][k][1], steps[1:]))
            return ("t", tuple(fields))

        root = self.vars[place.root.name]
        new = upd(root.sh, place.steps)
        if new is FAIL or not fits(new, root.env):
            return FAIL
        return new

    def current_shape(self, place):
        sh = self.vars[place.root.name].sh
        for s in place.steps:
            if not isinstance(sh, tuple):
                return sh
            sh = sh[3] if s[0] == "i" else sh[1][s[2]][1]
        return sh

    def changeable_roots(self, pred=None):
        return [v for v in self.vars.values()
                if v.can_change() and not v.freed and v.name not in self.nomut_roots and (pred is None or pred(v))]

    def rand_place(self, want, need_steps=False, roots=None, q=True):
        """A random changeable place whose type satisfies want."""
        cands = []
        for v in (roots if roots is not None else self.changeable_roots()):
            for p in self.place_paths(v, want, need_steps=need_steps):
                cands.append((v, p))
        if not cands:
            return None
        v, p = self.pick(cands)
        return self.build_place(v, p, q)

    # ---- expressions -------------------------------------------------------------------------

    def expr(self, t, env=None, d=2, q=True, mut=True, exp=False):
        """An expression of type t whose value fits env. q: quotes allowed (not inside `{}` of a
        string); mut: may change variables (pop, inout); exp: the expected type is known (`[]`)."""
        if env is None:
            env = self.top(t)
        prods = self.prods(t, d, mut)
        for _ in range(6):
            fn = self.wpick(prods)
            saved = self.save()
            e = fn(t, env, d, q, mut, exp)
            if e is not None:
                if self.has_shape(t) and not isinstance(e.sh, tuple):
                    e.sh = self.top(t)
                if fits(e.sh, env):
                    return e
            self.restore(saved)
        return self.lit(t, env, d, q, mut, exp)

    def sexpr(self, t, env=None, d=2, q=True, mut=True, exp=True):
        """An expression whose value is stored: an int is kept within +-VB."""
        e = self.expr(t, env, d, q, mut, exp)
        return self.wrap_int(e) if t == INT else e

    def prods(self, t, d, mut):
        deep = d > 0
        m = mut and deep
        if t == INT:
            return [(8, self.lit), (10, self.p_read), (6 if deep else 2, self.p_len), (3 if deep else 0, self.p_code),
                    (10 if deep else 0, self.p_arith), (2 if deep else 0, self.p_neg),
                    (2 if deep else 0, self.p_int_of_float), (2 if deep else 0, self.p_int_of_str),
                    (3 if deep else 0, self.p_index_of), (4 if m else 0, self.p_pop),
                    (4 if deep else 0, self.p_call), (3 if deep else 0, self.p_ifval)]
        if t == FLOAT:
            return [(8, self.lit), (3, self.p_special), (8, self.p_read), (8 if deep else 0, self.p_farith),
                    (2 if deep else 0, self.p_neg), (4 if deep else 0, self.p_float_of_int),
                    (3 if deep else 0, self.p_float_of_str), (3 if m else 0, self.p_pop),
                    (3 if deep else 0, self.p_call), (2 if deep else 0, self.p_ifval)]
        if t == BOOL:
            return [(4, self.lit), (6, self.p_read), (10 if deep else 0, self.p_cmp), (8 if deep else 0, self.p_eq),
                    (4 if deep else 0, self.p_logic), (2 if deep else 0, self.p_not),
                    (6 if deep else 0, self.p_contains), (3 if deep else 0, self.p_charpred),
                    (1 if m else 0, self.p_pop), (2 if deep else 0, self.p_call), (2 if deep else 0, self.p_ifval)]
        if t == STR:
            return [(10, self.lit), (8, self.p_read), (7 if deep else 0, self.p_interp), (6 if deep else 0, self.p_concat),
                    (5 if deep else 0, self.p_slice), (4 if deep else 0, self.p_replace), (4 if deep else 0, self.p_case),
                    (2 if deep else 0, self.p_repeat), (4 if deep else 0, self.p_join), (5 if deep else 0, self.p_str_of),
                    (3 if m else 0, self.p_pop), (3 if deep else 0, self.p_call), (3 if deep else 0, self.p_ifval)]
        if t == CHAR:
            return [(8, self.lit), (8, self.p_read), (6 if deep else 0, self.p_str_index), (4 if deep else 0, self.p_char_of),
                    (3 if deep else 0, self.p_char_case), (2 if m else 0, self.p_pop),
                    (2 if deep else 0, self.p_call), (2 if deep else 0, self.p_ifval)]
        if is_arr(t):
            el = t[1]
            return [(10, self.lit), (10, self.p_read), (5 if deep else 0, self.p_aconcat),
                    (4 if deep else 0, self.p_aslice), (2 if deep else 0, self.p_arepeat),
                    (5 if deep and el == STR else 0, self.p_split), (4 if deep and el == CHAR else 0, self.p_chars),
                    (3 if deep and el == INT else 0, self.p_codes), (2 if m else 0, self.p_pop),
                    (3 if deep else 0, self.p_call), (3 if deep else 0, self.p_ifval)]
        return [(8, self.lit), (10, self.p_read), (2 if m else 0, self.p_pop),
                (3 if deep else 0, self.p_call), (2 if deep else 0, self.p_ifval)]

    # literals --------------------------------------------------------------------------------

    def lit(self, t, env, d, q, mut, exp):
        r = self.r
        if t == INT:
            n = self.wpick([(10, r.randint(0, 9)), (4, r.randint(-9, -1)), (3, r.randint(10, 1000)),
                            (1, r.randint(-1000, -10)), (1, self.pick([2147483647, -2147483647, 65536, 1 << 40]))])
            return E(str(n), INT, iv=(n, n), atom=n >= 0)
        if t == FLOAT:
            if self.f["extreme_floats"] and self.chance(0.05):
                return E(self.pick(EXTREME_FLOATS), FLOAT)
            s = self.pick(FLOAT_POOL)
            neg = self.chance(0.2)
            return E(("-" if neg else "") + s, FLOAT, atom=not neg, fv=float(s))
        if t == BOOL:
            return E(self.pick(["true", "false"]), BOOL)
        if t == STR:
            lo, hi = env[1], env[2]
            if not q:
                digits = max(1, lo)
                n = self.r.randint(10 ** (digits - 1) if digits > 1 else 0, 10 ** digits - 1)
                return E(f"str({n})", STR, sh=("s", digits, digits))
            cands = [s for s in STR_POOL if lo <= len(s) <= hi]
            if cands and self.chance(0.9):
                s = self.pick(cands)
            else:
                n = self.r.randint(lo, max(lo, min(hi, lo + 4)))
                s = "".join(self.pick("abcxyz é😀") for _ in range(n))
            return E(str_lit(s), STR, sh=("s", len(s), len(s)))
        if t == CHAR:
            if self.f["nul_char"] and self.chance(0.03):
                return E("char(0)", CHAR)
            if not q:
                return E(f"char({self.pick([97, 98, 65, 48, 32, 233, 128512, 20013, 10])})", CHAR)
            return E(char_lit(self.pick(CHAR_POOL)), CHAR)
        if is_arr(t):
            lo, hi = env[1], env[2]
            if hi == 0:
                if exp:
                    return E("[]", t, sh=("a", 0, 0, "E"))
                el = self.lit(t[1], self.top(t[1]), 0, q, False, False)
                return E(f"[{el.code}].slice(0, 0)", t, sh=("a", 0, 0, "E"))
            n = self.r.randint(lo, max(lo, min(hi, 4 if d >= 2 else 2)))
            if n == 0 and not exp:
                n = 1
            if n == 0:
                return E("[]", t, sh=("a", 0, 0, "E"))
            els = [self.sexpr(t[1], env[3], max(0, d - 1), q=q, mut=mut, exp=exp) for _ in range(n)]
            sh = None
            if self.has_shape(t[1]):
                for e in els:
                    sh = e.sh if sh is None else join(sh, e.sh)
            return E("[" + ", ".join(e.code for e in els) + "]", t, sh=("a", n, n, sh))
        name = t[1]
        parts, shs = [], []
        for k, (f, ft) in enumerate(self.structs[name]):
            fe = self.sexpr(ft, env[1][k][1], max(0, d - 1), q=q, mut=mut)
            parts.append(f"{f}: {fe.code}")
            shs.append((f, fe.sh))
        return E(f"{name}({', '.join(parts)})", t, sh=("t", tuple(shs)))

    def p_special(self, t, env, d, q, mut, exp):
        code, atom = self.pick(SPECIAL_FLOATS)
        return E(code, FLOAT, atom=atom)

    # reads -----------------------------------------------------------------------------------

    def p_read(self, t, env, d, q, mut, exp):
        cands = []
        for v in self.live():
            for p in self.place_paths(v, lambda x: x == t):
                if is_struct(t) and p and p[-1][0] == "f" and not self.f["struct_field_reads"]:
                    continue
                cands.append((v, p))
        if not cands:
            return None
        v, p = self.pick(cands)
        pl = self.build_place(v, p, q)
        code = pl.code
        if p and self.f["risky"] and self.chance(0.01):
            # an index that may be out of bounds
            code = v.name
            for s in p:
                code = f"{code}[{self.expr(INT, None, 1, q=q, mut=False).code}]" if s[0] == "i" else f"{code}.{s[1]}"
        iv = None
        if t == INT:
            iv = v.iv if (not p and v.iv) else (-VB, VB)
        return E(code, t, sh=pl.sh, iv=iv)

    def any_seq(self, d, q, mut):
        t = STR if self.chance(0.5) else arr(self.rand_type(1))
        return self.expr(t, None, max(0, d - 1), q=q, mut=mut)

    def p_len(self, t, env, d, q, mut, exp):
        e = self.any_seq(d, q, mut)
        return E(f"{par(e)}.len()", INT, iv=(lo_of(e.sh), hi_of(e.sh)))

    def p_code(self, t, env, d, q, mut, exp):
        c = self.expr(CHAR, None, d - 1, q=q, mut=mut)
        return E(f"{par(c)}.code()", INT, iv=(0, 1114111))

    def p_arith(self, t, env, d, q, mut, exp):
        op = self.wpick([(5, "+"), (4, "-"), (4, "*"), (2, "/"), (2, "%")])
        a = self.expr(INT, None, d - 1, q=q, mut=mut)
        b = self.expr(INT, None, d - 1, q=q, mut=mut)
        if op in "+-":
            a, b = self.small_int(a, 2 ** 40), self.small_int(b, 2 ** 40)
            iv = (a.iv[0] + b.iv[0], a.iv[1] + b.iv[1]) if op == "+" else (a.iv[0] - b.iv[1], a.iv[1] - b.iv[0])
        elif op == "*":
            if mag(a.iv) * mag(b.iv) > SAFE:
                a = self.small_int(a, 1 << 20)
            if mag(a.iv) * mag(b.iv) > SAFE:
                b = self.small_int(b, 1 << 20)
            ps = [a.iv[0] * b.iv[0], a.iv[0] * b.iv[1], a.iv[1] * b.iv[0], a.iv[1] * b.iv[1]]
            iv = (min(ps), max(ps))
        else:
            if not (self.f["risky"] and self.chance(0.01)):
                b = self.nonzero(b)
            m = mag(a.iv)
            if op == "/":
                iv = (0, m) if a.iv[0] >= 0 and b.iv[0] > 0 else (-m, m)
            else:
                hi = min(m, max(1, mag(b.iv) - 1))
                iv = (0 if a.iv[0] >= 0 else -hi, 0 if a.iv[1] <= 0 else hi)
        return E(f"{par(a)} {op} {par(b)}", INT, iv=iv, atom=False)

    def p_neg(self, t, env, d, q, mut, exp):
        a = self.expr(t, None, d - 1, q=q, mut=mut)
        if t == INT:
            return E(f"-{par(a)}", INT, iv=(-a.iv[1], -a.iv[0]), atom=False)
        return E(f"-{par(a)}", FLOAT, atom=False, fv=a.fv)

    def p_int_of_float(self, t, env, d, q, mut, exp):
        f = self.expr(FLOAT, None, d - 1, q=q, mut=mut)
        if f.fv is not None and f.fv <= SAFE:
            m = int(f.fv) + 1
            return E(f"int({f.code})", INT, iv=(-m, m))
        if self.f["risky"] and self.chance(0.05):
            return E(f"int({f.code}) % 1000", INT, iv=(-999, 999), atom=False)
        return None

    def p_int_of_str(self, t, env, d, q, mut, exp):
        if q and self.chance(0.5):
            s = self.pick(NUM_STRS)
            return E(f"int({str_lit(s)})", INT, iv=(int(s), int(s)))
        if q and self.f["risky"] and self.chance(0.03):
            s = self.expr(STR, None, d - 1, q=q, mut=mut)
            return E(f"int({s.code}) % 1000", INT, iv=(-999, 999), atom=False)
        i = self.expr(INT, None, d - 1, q=q, mut=mut)
        return E(f"int(str({i.code}))", INT, iv=i.iv)

    def p_index_of(self, t, env, d, q, mut, exp):
        if self.chance(0.5):
            s = self.expr(STR, None, d - 1, q=q, mut=mut)
            x = self.expr(STR, None, d - 1, q=q, mut=mut)
            return E(f"{par(s)}.index_of({x.code})", INT, iv=(-1, hi_of(s.sh)))
        at = arr(self.rand_type(1))
        a = self.expr(at, None, d - 1, q=q, mut=mut)
        x = self.expr(at[1], None, d - 1, q=q, mut=mut)
        return E(f"{par(a)}.index_of({x.code})", INT, iv=(-1, max(0, hi_of(a.sh) - 1)))

    def p_pop(self, t, env, d, q, mut, exp):
        """`xs.pop()` / `xs.remove(i)` inside an expression."""
        pl = self.rand_place(lambda x: x == arr(t), q=q)
        if pl is None or not isinstance(pl.sh, tuple) or pl.sh[1] < 1:
            return None
        lo = pl.sh[1]
        if self.chance(0.6):
            code = f"{pl.code}.pop()"
        else:
            code = f"{pl.code}.remove({self.index(lo, None, q)})"
        el = pl.sh[3]
        new = self.update_place(pl, shrink_shape)
        if new is FAIL:
            return None
        self.vars[pl.root.name].sh = new
        self.nmut += 1
        return E(code, t, sh=el if isinstance(el, tuple) else None, iv=(-VB, VB) if t == INT else None)

    def p_call(self, t, env, d, q, mut, exp):
        fns = [f for f in self.callable() if f.ret == t and fits(f.ret_env, env)]
        if not mut:
            fns = [f for f in fns if not any(p.inout for p in f.params)]
        if not fns:
            return None
        f = self.pick(fns)
        code = self.call(f, d, q)
        if code is None:
            return None
        return E(code, t, sh=f.ret_env, iv=(-VB, VB) if t == INT else None)

    def p_ifval(self, t, env, d, q, mut, exp):
        c = self.expr(BOOL, None, d - 1, q=q, mut=mut)
        before = self.vstate()
        a = self.expr(t, env, d - 1, q=q, mut=mut)
        after_a = self.vstate()
        self.set_vstate(before)
        b = self.expr(t, env, d - 1, q=q, mut=mut)
        self.set_vstate(self.join_states(after_a, self.vstate()))
        sh = join(a.sh, b.sh) if self.has_shape(t) else None
        iv = (min(a.iv[0], b.iv[0]), max(a.iv[1], b.iv[1])) if t == INT else None
        fv = max(a.fv, b.fv) if t == FLOAT and a.fv is not None and b.fv is not None else None
        return E(f"if {c.code} {{ {a.code} }} else {{ {b.code} }}", t, sh=sh, iv=iv, atom=False, fv=fv)

    # floats ----------------------------------------------------------------------------------

    def p_farith(self, t, env, d, q, mut, exp):
        op = self.pick("+-*/")
        a = self.expr(FLOAT, None, d - 1, q=q, mut=mut)
        b = self.expr(FLOAT, None, d - 1, q=q, mut=mut)
        fv = None
        if a.fv is not None and b.fv is not None:
            if op in "+-":
                fv = a.fv + b.fv
            elif op == "*":
                fv = a.fv * b.fv
        return E(f"{par(a)} {op} {par(b)}", FLOAT, atom=False, fv=fv if fv is not None and fv < 1e300 else None)

    def p_float_of_int(self, t, env, d, q, mut, exp):
        i = self.expr(INT, None, d - 1, q=q, mut=mut)
        return E(f"float({i.code})", FLOAT, fv=float(mag(i.iv)))

    def p_float_of_str(self, t, env, d, q, mut, exp):
        if q and self.chance(0.5):
            s = self.pick(FLOAT_STRS + NUM_STRS)
            v = abs(float(s))
            return E(f"float({str_lit(s)})", FLOAT, fv=v if v < 1e300 else None)
        if self.chance(0.5):
            # NaN and -Infinity print as text that float() rejects: then a runtime error on both sides
            f = self.expr(FLOAT, None, d - 1, q=q, mut=mut)
            return E(f"float(str({f.code}))", FLOAT, fv=f.fv)
        i = self.expr(INT, None, d - 1, q=q, mut=mut)
        return E(f"float(str({i.code}))", FLOAT, fv=float(mag(i.iv)))

    # bools -----------------------------------------------------------------------------------

    def p_cmp(self, t, env, d, q, mut, exp):
        ct = self.pick([INT, INT, FLOAT, STR, CHAR])
        a = self.expr(ct, None, d - 1, q=q, mut=mut)
        b = self.expr(ct, None, d - 1, q=q, mut=mut)
        op = self.pick(["<", "<=", ">", ">=", "==", "!="])
        return E(f"{par(a)} {op} {par(b)}", BOOL, atom=False)

    def p_eq(self, t, env, d, q, mut, exp):
        """Equality of any type, often of values that are equal (copies, slices, the same variable)."""
        ct = self.rand_type()
        n0 = self.nmut
        a = self.expr(ct, None, d - 1, q=q, mut=mut)
        pure = self.nmut == n0
        if pure and self.chance(0.35):
            if is_arr(ct) and self.chance(0.4):
                b = E(f"{par(a)}.slice(0, {par(a)}.len())", ct, sh=a.sh)
            elif ct == STR and self.chance(0.4):
                b = E(f"{par(a)} + {str_lit('') if q else 'str(5).slice(0, 0)'}", ct, sh=a.sh, atom=False)
            else:
                b = E(a.code, ct, sh=a.sh, atom=a.atom)
        else:
            b = self.expr(ct, None, d - 1, q=q, mut=mut)
        op = self.pick(["==", "!="])
        return E(f"{par(a)} {op} {par(b)}", BOOL, atom=False)

    def p_logic(self, t, env, d, q, mut, exp):
        a = self.expr(BOOL, None, d - 1, q=q, mut=mut)
        before = self.vstate()
        b = self.expr(BOOL, None, d - 1, q=q, mut=mut)
        self.set_vstate(self.join_states(before, self.vstate()))
        op = self.pick(["&&", "||"])
        return E(f"{par(a)} {op} {par(b)}", BOOL, atom=False)

    def p_not(self, t, env, d, q, mut, exp):
        a = self.expr(BOOL, None, d - 1, q=q, mut=mut)
        return E(f"!{par(a)}", BOOL, atom=False)

    def p_contains(self, t, env, d, q, mut, exp):
        if self.chance(0.5):
            s = self.expr(STR, None, d - 1, q=q, mut=mut)
            x = self.expr(STR, None, d - 1, q=q, mut=mut)
            m = self.pick(["contains", "starts_with", "ends_with"])
            return E(f"{par(s)}.{m}({x.code})", BOOL)
        at = arr(self.rand_type(1))
        a = self.expr(at, None, d - 1, q=q, mut=mut)
        x = self.expr(at[1], None, d - 1, q=q, mut=mut)
        return E(f"{par(a)}.contains({x.code})", BOOL)

    def p_charpred(self, t, env, d, q, mut, exp):
        c = self.expr(CHAR, None, d - 1, q=q, mut=mut)
        m = self.pick(["is_digit", "is_letter", "is_upper", "is_lower", "is_space"])
        return E(f"{par(c)}.{m}()", BOOL)

    # strings ---------------------------------------------------------------------------------

    def p_interp(self, t, env, d, q, mut, exp):
        if not q:
            return None
        code = '"'
        lo = hi = 0
        for _ in range(self.r.randint(1, 3)):
            text = self.pick(["", "", " ", "x=", ", ", "é ", "[", "😀", "{", "}"])
            code += str_lit(text)[1:-1]
            lo += len(text)
            hi += len(text)
            pt = self.rand_type()
            e = self.expr(pt, None, d - 1, q=False, mut=mut)
            code += "{" + e.code + "}"
            hi += self.repr_len(pt, e.sh)
        tail = self.pick(["", "", "!", " end", "é"])
        code += str_lit(tail)[1:-1] + '"'
        return E(code, STR, sh=("s", lo + len(tail), hi + len(tail)))

    def p_concat(self, t, env, d, q, mut, exp):
        a = self.expr(STR, None, d - 1, q=q, mut=mut)
        b = self.expr(STR, None, d - 1, q=q, mut=mut)
        return E(f"{par(a)} + {par(b)}", STR, sh=("s", a.sh[1] + b.sh[1], a.sh[2] + b.sh[2]), atom=False)

    def p_slice(self, t, env, d, q, mut, exp):
        n0 = self.nmut
        s = self.expr(STR, None, d - 1, q=q, mut=mut)
        lo = s.sh[1]
        if self.f["risky"] and self.chance(0.02):
            a, b = self.r.randint(0, 3), self.r.randint(0, 5)
            return E(f"{par(s)}.slice({a}, {b})", STR, sh=("s", 0, max(0, b - a)))
        a = self.r.randint(0, lo)
        b = self.r.randint(a, lo)
        if self.chance(0.3) and self.nmut == n0 and s.atom:
            return E(f"{s.code}.slice({a}, {s.code}.len())", STR, sh=("s", lo - a, max(0, s.sh[2] - a)))
        return E(f"{par(s)}.slice({a}, {b})", STR, sh=("s", b - a, b - a))

    def sep(self, q, nonempty=True):
        if q:
            s = self.pick(SEP_POOL)
            if not nonempty and self.chance(0.1):
                s = ""
            return E(str_lit(s), STR, sh=("s", len(s), len(s)))
        c = self.expr(CHAR, None, 0, q=False, mut=False)
        return E(f"str({c.code})", STR, sh=("s", 1, 1))

    def p_replace(self, t, env, d, q, mut, exp):
        s = self.expr(STR, None, d - 1, q=q, mut=mut)
        old = self.sep(q)
        if self.f["risky"] and q and self.chance(0.02):
            old = E('""', STR, sh=("s", 0, 0))
        new = self.expr(STR, None, max(0, d - 2), q=q, mut=mut)
        return E(f"{par(s)}.replace({old.code}, {new.code})", STR, sh=("s", 0, s.sh[2] * max(1, new.sh[2])))

    def p_case(self, t, env, d, q, mut, exp):
        s = self.expr(STR, None, d - 1, q=q, mut=mut)
        m = self.pick(["trim", "upper", "lower"])
        return E(f"{par(s)}.{m}()", STR, sh=("s", 0, s.sh[2]) if m == "trim" else s.sh)

    def p_repeat(self, t, env, d, q, mut, exp):
        s = self.expr(STR, None, d - 1, q=q, mut=mut)
        k = self.r.randint(0, 3)
        if self.f["risky"] and self.chance(0.02):
            k = -1
        return E(f"{par(s)}.repeat({k})", STR, sh=("s", s.sh[1] * max(k, 0), s.sh[2] * max(k, 0)))

    def p_join(self, t, env, d, q, mut, exp):
        at = arr(self.pick([STR, STR, CHAR]))
        a = self.expr(at, None, d - 1, q=q, mut=mut)
        sep = self.sep(q, nonempty=False)
        n = a.sh[2]
        each = a.sh[3][2] if at[1] == STR and n > 0 and isinstance(a.sh[3], tuple) else 1
        return E(f"{par(a)}.join({sep.code})", STR, sh=("s", 0, n * each + max(0, n - 1) * sep.sh[2]))

    def p_str_of(self, t, env, d, q, mut, exp):
        pt = self.rand_type()
        e = self.expr(pt, None, d - 1, q=q, mut=mut)
        return E(f"str({e.code})", STR, sh=("s", 0, self.repr_len(pt, e.sh)))

    # chars -----------------------------------------------------------------------------------

    def p_str_index(self, t, env, d, q, mut, exp):
        n0 = self.nmut
        s = self.expr(STR, ("s", 1, 10 ** 9), d - 1, q=q, mut=mut)
        if s.sh[1] < 1:
            return None
        prefix = s.code if s.atom and self.nmut == n0 and len(s.code) < 40 else None
        return E(f"{par(s)}[{self.index(s.sh[1], prefix, q)}]", CHAR)

    def p_char_of(self, t, env, d, q, mut, exp):
        k = self.r.random()
        if k < 0.3:
            c = self.expr(CHAR, None, d - 1, q=q, mut=mut)
            return E(f"char({par(c)}.code())", CHAR)
        if k < 0.6:
            n = self.pick([65, 97, 122, 48, 32, 10, 233, 223, 937, 8364, 20013, 65370, 57344, 128512, 127881, 1114111])
            return E(f"char({n})", CHAR)
        e = self.expr(INT, None, d - 1, q=q, mut=mut)
        if self.f["risky"] and self.chance(0.03):
            return E(f"char({e.code})", CHAR)
        base = self.pick([97, 65, 48, 192, 128512])
        if e.iv[0] >= 0:
            return E(f"char({par(e)} % 26 + {base})", CHAR)
        return E(f"char(({par(e)} % 26 + 26) % 26 + {base})", CHAR)

    def p_char_case(self, t, env, d, q, mut, exp):
        c = self.expr(CHAR, None, d - 1, q=q, mut=mut)
        return E(f"{par(c)}.{self.pick(['upper', 'lower'])}()", CHAR)

    # arrays ----------------------------------------------------------------------------------

    def p_aconcat(self, t, env, d, q, mut, exp):
        a = self.expr(t, None, d - 1, q=q, mut=mut)
        if exp and self.chance(0.1):
            return E(f"{par(a)} + []", t, sh=a.sh, atom=False)
        b = self.expr(t, None, d - 1, q=q, mut=mut)
        return E(f"{par(a)} + {par(b)}", t, sh=grow_shape(a.sh, b.sh), atom=False)

    def p_aslice(self, t, env, d, q, mut, exp):
        a = self.expr(t, None, d - 1, q=q, mut=mut)
        lo = a.sh[1]
        if self.f["risky"] and self.chance(0.02):
            x, y = self.r.randint(0, 3), self.r.randint(0, 4)
            return E(f"{par(a)}.slice({x}, {y})", t, sh=("a", 0, max(0, y - x), a.sh[3] if y > x else "E"))
        x = self.r.randint(0, lo)
        y = self.r.randint(x, lo)
        return E(f"{par(a)}.slice({x}, {y})", t, sh=("a", y - x, y - x, a.sh[3] if y > x else "E"))

    def p_arepeat(self, t, env, d, q, mut, exp):
        a = self.expr(t, None, d - 1, q=q, mut=mut)
        k = self.r.randint(0, 3)
        return E(f"{par(a)}.repeat({k})", t, sh=("a", a.sh[1] * k, a.sh[2] * k, a.sh[3] if k and a.sh[2] else "E"))

    def p_split(self, t, env, d, q, mut, exp):
        s = self.expr(STR, None, d - 1, q=q, mut=mut)
        sep = self.sep(q)
        if self.f["risky"] and q and self.chance(0.02):
            sep = E('""', STR, sh=("s", 0, 0))
        return E(f"{par(s)}.split({sep.code})", t, sh=("a", 1, s.sh[2] + 1, ("s", 0, s.sh[2])))

    def p_chars(self, t, env, d, q, mut, exp):
        s = self.expr(STR, None, d - 1, q=q, mut=mut)
        return E(f"{par(s)}.chars()", t, sh=("a", s.sh[1], s.sh[2], None if s.sh[2] else "E"))

    def p_codes(self, t, env, d, q, mut, exp):
        s = self.expr(STR, None, d - 1, q=q, mut=mut)
        return E(f"{par(s)}.codes()", t, sh=("a", s.sh[1], s.sh[2], None if s.sh[2] else "E"))

    # calls -----------------------------------------------------------------------------------

    def callable(self):
        return [f for f in self.fns if f.index < self.cur.index]

    def call(self, f, d, q):
        """The code of a call of f: arguments left to right; an `inout` place may get any value
        within the parameter's envelope."""
        args, roots, pending = [], [], []
        for p in f.params:
            if p.inout:
                ok = []
                for v in self.changeable_roots(lambda v: v.name not in roots):
                    for path in self.place_paths(v, lambda x: x == p.ty):
                        pl = self.build_place(v, path, q)
                        if fits(pl.sh, p.env) and fits(p.env, pl.env):
                            ok.append(pl)
                if not ok:
                    return None
                pl = self.pick(ok)
                roots.append(pl.root.name)
                args.append("inout " + pl.code)
                pending.append((pl, p.env))
            else:
                same = [n for n in roots if self.vars[n].ty == p.ty and fits(self.vars[n].sh, p.env)]
                if same and self.chance(0.4):
                    # a plain argument next to an `inout` one of the same variable: a snapshot
                    args.append(same[0])
                else:
                    args.append(self.sexpr(p.ty, p.env, max(0, d - 1), q=q).code)
        for pl, penv in pending:
            new = self.update_place(pl, lambda s, penv=penv: penv)
            if new is FAIL:
                return None
            self.vars[pl.root.name].sh = new
            self.nmut += 1
        return f"{f.name}({', '.join(args)})"

    # ---- statements --------------------------------------------------------------------------

    def block(self, nmin, nmax, final=None):
        """A block of statements in a new scope: (lines, whether it ends with a jump)."""
        self.scopes.append([])
        lines, term = [], False
        for _ in range(self.r.randint(nmin, nmax)):
            if self.budget <= 0:
                break
            ls, term = self.stmt()
            lines += ls
            if term:
                break
        if final is not None and not term:
            ls, term = final()
            lines += ls
        for name in self.scopes.pop():
            self.vars.pop(name, None)
            if name not in self.dead:
                self.dead.append(name)
        return lines, term

    def stmt(self):
        self.budget -= 1
        in_fn = self.cur.name != "main"
        big = self.budget > 3
        opts = [
            (16, self.s_decl), (13, self.s_print), (6, self.s_copy_mutate), (5, self.s_assign),
            (6, self.s_compound), (9, self.s_store), (10, self.s_method), (7 if big else 1, self.s_if),
            (3 if big else 0, self.s_while), (4 if big else 0, self.s_for_range), (4 if big else 0, self.s_for_each),
            (5 if self.callable() else 0, self.s_call), (2 if self.f["free"] else 0, self.s_free),
            (1 if self.f["keep"] else 0, self.s_keep), (2 if self.f["arena"] and big else 0, self.s_arena),
            (4 if self.loop_depth else 0, self.s_jump), (2 if in_fn else 0.3, self.s_ret_early),
            (3 if self.f["evalorder"] else 0, self.s_evalorder),
        ]
        for _ in range(12):
            fn = self.wpick(opts)
            saved = self.save()
            self.nomut_roots = ()
            res = fn()
            self.nomut_roots = ()
            if res is not None:
                return res
            self.restore(saved)
        return [f"print({self.expr(INT, None, 1).code})"], False

    def indent(self, lines):
        return ["    " + l for l in lines]

    # declarations and prints -----------------------------------------------------------------

    def s_decl(self):
        t = self.rand_type()
        kind = "var" if self.chance(0.7) else "let"
        if is_arr(t) and self.chance(0.12):
            name = self.new_var_name()
            self.declare(name, t, kind, ("a", 0, 0, "E"))
            return [f"{kind} {name}: {self.tname(t)} = []"], False
        annot = self.chance(0.2)
        e = self.sexpr(t, self.top(t), self.r.randint(1, 3), exp=annot)
        name = self.new_var_name()
        self.declare(name, t, kind, e.sh)
        ann = f": {self.tname(t)}" if annot else ""
        return [f"{kind} {name}{ann} = {e.code}"], False

    def s_print(self):
        vs = self.live()
        if vs and self.chance(0.45):
            return [f"print({self.pick(vs).name})"], False
        e = self.expr(self.rand_type(), None, self.r.randint(1, 3))
        return [f"print({e.code})"], False

    def s_copy_mutate(self):
        """`var b = a`, change one of them, print both: copies must never share changes."""
        srcs = self.live(lambda v: self.has_shape(v.ty))
        if not srcs:
            return None
        a = self.pick(srcs)
        name = self.new_var_name()
        lines = [f"var {name} = {a.name}"]
        self.declare(name, a.ty, "var", a.sh)
        target = a.name if a.can_change() and self.chance(0.4) else name
        for _ in range(self.r.randint(1, 2)):
            res = self.mutation_of(target)
            if res is None:
                break
            lines += res
        lines += [f"print({a.name})", f"print({name})"]
        return lines, False

    def mutation_of(self, name):
        """One statement that changes a part of the variable `name`."""
        v = self.vars.get(name)
        if v is None or not v.can_change() or v.freed:
            return None
        k = self.r.random()
        saved = self.save()
        if k < 0.45:
            r = self.s_method(roots=[v])
        elif k < 0.85:
            r = self.s_store(roots=[v])
        else:
            r = self.s_compound(roots=[v])
        self.nomut_roots = ()
        if r is None:
            self.restore(saved)
            return None
        return r[0]

    # assignments -----------------------------------------------------------------------------

    def s_assign(self):
        vs = [v for v in self.vars.values() if v.can_change() and (not v.freed or v.kind == "var")]
        if not vs:
            return None
        name = self.pick(vs).name
        e = self.sexpr(self.vars[name].ty, self.vars[name].env, self.r.randint(1, 3))
        v = self.vars[name]
        v.sh = e.sh
        v.freed = False
        return [f"{name} = {e.code}"], False

    def guard_grow(self, pl, stmt, add, cap):
        """`stmt` adds add's elements (or characters) to the place: unguarded when it surely
        stays within the cap, otherwise inside `if place.len() <= k`."""
        new = self.update_place(pl, lambda s: grow_shape(s, add))
        if new is not FAIL:
            self.vars[pl.root.name].sh = new
            return [stmt]
        k = cap - add[2]
        if k < 0:
            return None
        new = self.update_place(pl, lambda s: grow_shape(with_hi(s, min(s[2], k)), add))
        if new is FAIL:
            return None
        self.vars[pl.root.name].sh = join(self.vars[pl.root.name].sh, new)
        return [f"if {pl.code}.len() <= {k} {{", "    " + stmt, "}"]

    def s_compound(self, roots=None):
        pl = self.rand_place(lambda t: t in (INT, FLOAT, STR) or is_arr(t), roots=roots)
        if pl is None:
            return None
        self.nomut_roots = (pl.root.name,)
        t = pl.ty
        if t == INT:
            op = self.wpick([(5, "+="), (4, "-="), (1, "*="), (2, "/="), (2, "%=")])
            if op == "*=":
                code = self.pick(["-1", "1", "0"])
            elif op in ("/=", "%="):
                code = self.nonzero(self.expr(INT, None, 1)).code
            else:
                code = self.small_int(self.expr(INT, None, 2), 1 << 16).code
            return [f"{pl.code} {op} {code}"], False
        if t == FLOAT:
            e = self.expr(FLOAT, None, 2)
            return [f"{pl.code} {self.pick(['+=', '-=', '*=', '/='])} {e.code}"], False
        e = self.expr(t, None, 2)  # `+=` does not pass the expected type on (`xs += []` is E0230)
        lines = self.guard_grow(pl, f"{pl.code} += {e.code}", e.sh, SCAP if t == STR else ACAP)
        return None if lines is None else (lines, False)

    def s_store(self, roots=None):
        pl = self.rand_place(lambda t: True, need_steps=True, roots=roots)
        if pl is None:
            return None
        self.nomut_roots = (pl.root.name,)
        t = pl.ty
        if t in (INT, FLOAT) and self.chance(0.4):
            if t == INT:
                op = self.pick(["+=", "-="])
                code = self.small_int(self.expr(INT, None, 2), 1 << 16).code
            else:
                op = self.pick(["+=", "-=", "*=", "/="])
                code = self.expr(FLOAT, None, 2).code
            return [f"{pl.code} {op} {code}"], False
        if t == STR and self.chance(0.3):
            e = self.expr(STR, None, 1)
            lines = self.guard_grow(pl, f"{pl.code} += {e.code}", e.sh, SCAP)
            return None if lines is None else (lines, False)
        e = self.sexpr(t, pl.env, self.r.randint(1, 3))
        new = self.update_place(pl, lambda s: e.sh)
        if new is FAIL:
            return None
        self.vars[pl.root.name].sh = new
        return [f"{pl.code} = {e.code}"], False

    def s_method(self, roots=None):
        pl = self.rand_place(is_arr, roots=roots)
        if pl is None or not isinstance(pl.sh, tuple):
            return None
        self.nomut_roots = (pl.root.name,)
        t, el, sh, floor = pl.ty, pl.ty[1], pl.sh, pl.env[1]
        m = self.wpick([(8, "push"), (3, "insert"), (5, "pop"), (3, "remove"),
                        (4 if el in (INT, FLOAT, STR, CHAR) else 0, "sort"), (2, "reverse"), (2, "+=")])
        if m in ("sort", "reverse"):
            return [f"{pl.code}.{m}()"], False
        if m == "+=":
            e = self.expr(t, pl.env, 2)
            lines = self.guard_grow(pl, f"{pl.code} += {e.code}", e.sh, ACAP)
            return None if lines is None else (lines, False)
        if m in ("push", "insert"):
            e = self.sexpr(el, pl.env[3], 2)
            if m == "push":
                stmt = f"{pl.code}.push({e.code})"
            else:
                ix = f"{pl.code}.len()" if self.chance(0.3) else str(self.r.randint(0, sh[1]))
                stmt = f"{pl.code}.insert({ix}, {e.code})"
            lines = self.guard_grow(pl, stmt, ("a", 1, 1, e.sh), ACAP)
            return None if lines is None else (lines, False)
        # pop / remove: one element less
        if sh[1] >= 1 and sh[1] - 1 >= floor:
            if m == "pop":
                stmt = f"{pl.code}.pop()"
            else:
                stmt = f"{pl.code}.remove({self.index(sh[1], None, True)})"
            if self.chance(0.4):
                stmt = f"print({stmt})"
            new = self.update_place(pl, shrink_shape)
            if new is FAIL:
                return None
            self.vars[pl.root.name].sh = new
            return [stmt], False
        if self.f["risky"] and floor == 0 and self.chance(0.02):
            # may pop an empty array
            new = self.update_place(pl, shrink_shape)
            if new is FAIL:
                return None
            self.vars[pl.root.name].sh = new
            return [f"{pl.code}.pop()"], False
        k = max(floor, 0)
        stmt = f"{pl.code}.pop()" if m == "pop" else f"{pl.code}.remove({self.r.randint(0, k)})"
        new = self.update_place(pl, lambda s: shrink_shape(with_lo(s, max(s[1], k + 1))) if s[2] > k else s)
        if new is FAIL:
            return None
        self.vars[pl.root.name].sh = join(self.vars[pl.root.name].sh, new)
        return [f"if {pl.code}.len() > {k} {{", "    " + stmt, "}"], False

    def s_evalorder(self):
        """Statements whose operands change the array they read: left to right matters."""
        pl = self.rand_place(lambda t: is_arr(t) and t[1] in (INT, STR), q=False)
        if pl is None or not isinstance(pl.sh, tuple) or pl.sh[1] < 2 or pl.sh[1] - 2 < pl.env[1]:
            return None
        x, s = pl.code, pl.ty[1] == STR
        pop = f"{x}.pop().len()" if s else f"{x}.pop()"
        forms = [
            (f"print({x}.len() + {pop})", 1),
            (f"print([{x}.len(), {pop}, {x}.len()])", 1),
            (f'print("{{{x}}} {{{x}.pop()}} {{{x}}}")', 1),
            (f"{x}[0] = {x}.pop()", 1),
            (f"{x}.push({x}.pop())", 0),
            (f"{x}.insert(0, {x}.remove(1))", 0),
            (f"print({x} + [{x}.pop()])", 1),
            (f"print({x}.remove(0) + {x}.remove(0))", 2),
            (f"print({x} == {x}.slice(0, {pop} % 1 + {x}.len()))", 1),
            (f"print({x}.slice(0, {pop} % 1 + {x}.len()))", 1),
        ]
        if not s:
            forms += [(f"{x}[0] += {x}.pop()", 1), (f"{x}[{x}.len() - 2] -= {x}.remove(0)", 1),
                      (f"{x}[1] = {x}[0] + {x}.remove(0)", 1)]
        code, n = self.pick(forms)
        new = self.update_place(pl, lambda sh: ("a", sh[1] - n, sh[2], sh[3]))
        if new is FAIL:
            return None
        self.vars[pl.root.name].sh = new
        return [code], False

    # control flow ----------------------------------------------------------------------------

    def s_if(self):
        n_elif = self.wpick([(6, 0), (2, 1), (1, 2)])
        has_else = self.chance(0.55)
        cond = self.expr(BOOL, None, 2)
        lines = [f"if {cond.code} {{"]
        ends = []
        for i in range(n_elif + 1):
            before = self.vstate()
            body, term = self.block(1, 3)
            lines += self.indent(body)
            if not term:
                ends.append(self.vstate())
            self.set_vstate(before)
            if i < n_elif:
                lines.append(f"}} else if {self.expr(BOOL, None, 2).code} {{")
        if has_else:
            lines.append("} else {")
            body, term = self.block(1, 3)
            lines += self.indent(body)
            if not term:
                ends.append(self.vstate())
        else:
            ends.append(self.vstate())
        lines.append("}")
        if not ends:
            return lines, True
        st = ends[0]
        for s in ends[1:]:
            st = self.join_states(st, s)
        self.set_vstate(st)
        return lines, False

    def enter_loop(self):
        """Chooses which outer variables the loop body may change and forgets what is known about
        them (the body may run any number of times); the others are frozen in the body."""
        saved = {}
        for v in self.vars.values():
            saved[v.name] = (v.env, v.frozen)
            if not v.can_change():
                continue
            if not self.has_shape(v.ty):
                continue
            if self.chance(0.6):
                if isinstance(v.sh, tuple) and v.sh[0] in "sa" and self.chance(0.6):
                    # it may grow but never gets shorter than it is now
                    v.env = with_lo(v.env, max(v.env[1], v.sh[1]))
                v.sh = v.env
            else:
                v.frozen = True
        self.loop_depth += 1
        return saved

    def exit_loop(self, saved, entry):
        self.loop_depth -= 1
        for name, (env, frozen) in saved.items():
            v = self.vars.get(name)
            if v is None:
                continue
            if v.can_change() and self.has_shape(v.ty):
                v.sh = v.env
            v.env, v.frozen = env, frozen
            if name in entry:
                v.freed = entry[name].freed

    def loop_body(self, var=None):
        entry = self.vstate()
        saved = self.enter_loop()
        self.scopes.append([])
        if var is not None:
            self.declare(*var[:4], env=var[4] if len(var) > 4 else None, iv=var[5] if len(var) > 5 else None)
        body, _ = self.block(1, 4)
        for n in self.scopes.pop():
            self.vars.pop(n, None)
        self.exit_loop(saved, entry)
        return body

    def s_while(self):
        k = self.r.randint(1, 5)
        cnt = self.fresh("w")
        self.declare(cnt, INT, "counter", None, iv=(0, k))
        cond = f"{cnt} < {k}"
        entry = self.vstate()
        saved = self.enter_loop()
        if self.chance(0.3):
            cond += f" && {par(self.expr(BOOL, None, 1, mut=False))}"
        self.scopes.append([])
        body, _ = self.block(1, 4)
        self.scopes.pop()
        self.exit_loop(saved, entry)
        return [f"var {cnt} = 0", f"while {cond} {{", f"    {cnt} += 1"] + self.indent(body) + ["}"], False

    def s_for_range(self):
        k = self.r.random()
        if k < 0.5:
            a = self.r.randint(0, 2)
            b = a + self.r.randint(0, 5)
            head, iv = f"{a}..{b}", (a, b - 1)
        elif k < 0.8:
            seqs = self.live(lambda v: v.ty == STR or is_arr(v.ty))
            seqs = [v for v in seqs if hi_of(v.sh) <= 8]
            if not seqs:
                return None
            v = self.pick(seqs)
            head, iv = f"0..{v.name}.len()", (0, max(0, hi_of(v.sh) - 1))
        else:
            a = self.expr(INT, None, 1, mut=False)
            n = self.r.randint(0, 4)
            head, iv = f"{par(a)}..{par(a)} + {n}", (a.iv[0], a.iv[1] + n - 1)
        name = self.fresh("i")
        body = self.loop_body((name, INT, "loop", None, None, iv))
        return [f"for {name} in {head} {{"] + self.indent(body) + ["}"], False

    def s_for_each(self):
        if self.chance(0.6):
            seqs = self.live(lambda v: (v.ty == STR or is_arr(v.ty)) and isinstance(v.sh, tuple))
            if not seqs:
                return None
            v = self.pick(seqs)
            it = E(v.name, v.ty, sh=v.sh)
        else:
            t = STR if self.chance(0.3) else arr(self.rand_type(1))
            it = self.expr(t, None, 2)
        if it.sh[2] > 12:
            return None
        et = CHAR if it.ty == STR else it.ty[1]
        esh = it.sh[3] if it.ty != STR and it.sh[2] > 0 and isinstance(it.sh[3], tuple) else None
        if self.has_shape(et) and esh is None:
            esh = self.top(et)
        name = self.new_var_name()
        body = self.loop_body((name, et, "loop", esh, esh))
        return [f"for {name} in {it.code} {{"] + self.indent(body) + ["}"], False

    def s_jump(self):
        if self.loop_depth == 0:
            return None
        word = self.pick(["break", "continue"])
        if self.chance(0.85):
            return [f"if {self.expr(BOOL, None, 2).code} {{", f"    {word}", "}"], False
        return [word], True

    def s_ret_early(self):
        f = self.cur
        if f.ret is None:
            if f.name == "main" and not self.chance(0.2):
                return None
            return [f"if {self.expr(BOOL, None, 2).code} {{", "    ret", "}"], False
        c = self.expr(BOOL, None, 2)
        return [f"if {c.code} {{", f"    ret {self.ret_expr().code}", "}"], False

    def ret_expr(self):
        return self.sexpr(self.cur.ret, self.cur.ret_env, 2)

    # calls, memory, arenas ---------------------------------------------------------------------

    def s_call(self):
        fns = self.callable()
        if not fns:
            return None
        f = self.pick(fns)
        code = self.call(f, 2, True)
        if code is None:
            return None
        if f.ret is not None and self.chance(0.5):
            return [f"print({code})"], False
        return [code], False

    def free_targets(self):
        return self.live(lambda v: v.kind in ("let", "var") and self.managed(v.ty) and v.arena == self.arena_depth
                         and not v.frozen)

    def s_free(self):
        vs = self.free_targets()
        if not vs:
            return None
        v = self.pick(vs)
        name = v.name
        outer = v.ld < self.loop_depth
        if outer and v.kind != "var":
            return None
        self.vars[name].freed = True
        lines = [f"free({name})"]
        if outer or (v.kind == "var" and self.chance(0.4)):
            # inside a loop an outer variable gets a new value right away (the next round reads it)
            e = self.sexpr(v.ty, v.env, 2)
            self.vars[name].freed = False
            self.vars[name].sh = e.sh
            lines.append(f"{name} = {e.code}")
        return lines, False

    def s_keep(self):
        vs = self.free_targets()
        if not vs:
            return None
        return [f"keep({self.pick(vs).name})"], False

    def s_arena(self):
        saved = {}
        for v in self.vars.values():
            saved[v.name] = v.frozen
            if self.managed(v.ty):
                v.frozen = True
        self.arena_depth += 1
        body, term = self.block(1, 4)
        self.arena_depth -= 1
        for name, fr in saved.items():
            if name in self.vars:
                self.vars[name].frozen = fr
        return ["arena {"] + self.indent(body) + ["}"], term

    # ---- functions and the program -----------------------------------------------------------

    def param_env(self, t):
        env = self.top(t)
        if (t == STR or is_arr(t)) and self.chance(0.5):
            env = with_lo(env, self.r.randint(1, 2))
        return env

    def gen_fn(self, idx):
        name = None
        if self.f["exotic_names"] and self.chance(0.15):
            free = [n for n in EXOTIC_NAMES if n not in self.used_names]
            if free:
                name = self.pick(free)
        if name is None:
            name = self.fresh(self.pick(FN_BASES))
        self.used_names.add(name)
        params = []
        for _ in range(self.wpick([(2, 0), (5, 1), (4, 2), (2, 3)])):
            t = self.rand_type()
            inout = self.f["inout"] and self.chance(0.3)
            params.append(Param(self.new_var_name(), t, inout, self.param_env(t)))
        ret = None if self.chance(0.3) else self.rand_type()
        ret_env = None
        if ret is not None:
            ret_env = self.top(ret)
            if (ret == STR or is_arr(ret)) and self.chance(0.4):
                ret_env = with_lo(ret_env, 1)
        f = Fn(name, params, ret, ret_env, idx)
        self.cur = f
        self.vars, self.scopes, self.dead = {}, [[]], []
        self.loop_depth = self.arena_depth = 0
        for p in params:
            self.declare(p.name, p.ty, "inout" if p.inout else "param", p.env, env=p.env)
        sig = ", ".join(("inout " if p.inout else "") + f"{p.name}: {self.tname(p.ty)}" for p in params)
        head = f"fn {name}({sig})" + (f" -> {self.tname(ret)}" if ret else "")
        if ret is not None and self.chance(0.2):
            e = self.ret_expr()
            self.fns.append(f)
            return [f"{head} = {e.code}"]
        self.budget = self.r.randint(4, 14)

        def final():
            if ret is None:
                return [], False
            if self.f["arena"] and self.chance(0.15):
                self.arena_depth += 1
                for v in self.vars.values():
                    if self.managed(v.ty):
                        v.frozen = True
                e = self.ret_expr()
                self.arena_depth -= 1
                return ["arena {", f"    ret {e.code}", "}"], True
            return [f"ret {self.ret_expr().code}"], True

        body, _ = self.block(2, 6, final=final)
        self.fns.append(f)
        return [head + " {"] + self.indent(body) + ["}"]

    def final_prints(self):
        return [f"print({v.name})" for v in self.vars.values() if not v.freed and self.chance(0.8)], False

    def program(self):
        lines = self.gen_structs()
        if lines:
            lines.append("")
        for i in range(self.wpick([(2, 0), (3, 1), (3, 2), (3, 3), (2, 4)])):
            lines += self.gen_fn(i)
            lines.append("")
        self.cur = Fn("main", [], None, None, len(self.fns))
        self.vars, self.scopes, self.dead = {}, [[]], []
        self.loop_depth = self.arena_depth = 0
        self.budget = self.r.randint(15, 40)
        body, _ = self.block(8, 30, final=self.final_prints)
        lines += ["fn main() {"] + self.indent(body) + ["}"]
        return "\n".join(lines) + "\n"


def generate(seed, feats=None):
    return Gen(seed, feats).program()


# ---------------------------------------------------------------------------------------------
# The oracle: build and run on both backends, compare
# ---------------------------------------------------------------------------------------------

RUN_TIMEOUT = 20
BUILD_TIMEOUT = 180


class Result:
    """One program built for one backend (with or without the IR optimizer) and run."""
    __slots__ = ("target", "opt", "build_rc", "build_err", "rc", "out", "err")

    def __init__(self, target, opt):
        self.target, self.opt = target, opt
        self.build_rc = self.rc = None
        self.build_err = self.out = self.err = b""

    @property
    def label(self):
        return ("native" if self.target == "c" else "js") + ("" if self.opt else " NYRA_OPT=0")


def run_cmd(cmd, env, timeout, cwd=None):
    try:
        p = subprocess.run(cmd, env=env, capture_output=True, timeout=timeout, cwd=cwd)
        return p.returncode, p.stdout, p.stderr
    except subprocess.TimeoutExpired as ex:
        return None, ex.stdout or b"", ex.stderr or b""


def build_and_run(src, target, opt, workdir):
    """Builds workdir/src for target ("c" or "js") and runs it."""
    res = Result(target, opt)
    env = dict(os.environ)
    for k in ("NYRA_OPT", "NYRA_LEAKCHECK", "NYRA_JSON", "NYRA_DUMP"):
        env.pop(k, None)
    if not opt:
        env["NYRA_OPT"] = "0"
    stem = os.path.splitext(src)[0]
    out = f"{stem}-{target}{'' if opt else '0'}" + (".js" if target == "js" else EXE)
    if os.path.exists(os.path.join(workdir, out)):
        os.remove(os.path.join(workdir, out))
    cmd = [NYRA, "build", src, "-o", out] + (["--js"] if target == "js" else [])
    res.build_rc, _, res.build_err = run_cmd(cmd, env, BUILD_TIMEOUT, cwd=workdir)
    if res.build_rc != 0:
        return res
    renv = dict(os.environ)
    for k in ("NYRA_OPT", "NYRA_LEAKCHECK", "NYRA_JSON", "NYRA_DUMP"):
        renv.pop(k, None)
    if target == "c":
        renv["NYRA_LEAKCHECK"] = "1"
        cmd = [os.path.join(workdir, out)]
    else:
        cmd = ["node", out]
    res.rc, res.out, res.err = run_cmd(cmd, renv, RUN_TIMEOUT, cwd=workdir)
    return res


def text(b, native=True):
    """Output as text. A native program on Windows writes stdout and stderr in text mode, so each
    `\n` arrives as `\r\n`: that is undone. Node writes the bytes as they are."""
    s = b.decode("utf-8", "replace")
    return s.replace("\r\n", "\n") if native and os.name == "nt" else s


def out_text(r):
    return text(r.out, r.target == "c")


def err_text(r):
    return text(r.err, r.target == "c")


def err_head(r):
    """The first stderr line and the `-->` position line of a runtime error."""
    lines = err_text(r).replace("\r\n", "\n").split("\n")
    pos = next((l.strip() for l in lines if l.strip().startswith("-->")), "")
    return lines[0] + " " + pos


def build_failure(r):
    t = text(r.build_err)
    if r.build_rc is None:
        return "timeout-build", "the build timed out"
    m = re.search(r"panicked at ([^\n]*)\n([^\n]*)", t)
    if m:
        return "crash-panic", f"{m.group(1)} {m.group(2)}".strip()
    for key, kind in (("internal error", "crash-ir"), ("not supported yet", "crash-unsupported")):
        if key in t:
            return kind, next(l for l in t.split("\n") if key in l).strip()
    if "failed to compile the generated C" in t:
        errs = [l.split("error:", 1)[1].strip() for l in t.split("\n") if "error:" in l]
        return "crash-cc", "gcc: " + errs[0] if errs else t.strip().split("\n")[-1]
    m = re.search(r"error\[(E\d+)\]: ([^\n]*)\n\s*--> ([^\n]*)", t)
    if m:
        return "gen-typeerror", f"{m.group(1)} {m.group(2)} at {m.group(3)}"
    return "crash-build", t.strip().split("\n")[0] if t.strip() else f"exit {r.build_rc}"


def js_error(r):
    t = err_text(r)
    m = re.search(r"^(\w*Error)\b[^\n]*", t, re.M)
    return m.group(0) if m else (t.strip().split("\n")[-1] if t.strip() else f"exit {r.rc}")


def first_diff(a, b):
    la, lb = out_text(a).split("\n"), out_text(b).split("\n")
    for i in range(max(len(la), len(lb))):
        x = la[i] if i < len(la) else "<missing>"
        y = lb[i] if i < len(lb) else "<missing>"
        if x != y:
            return i + 1, x[:300], y[:300]
    return 0, "", ""


class Failure:
    __slots__ = ("kind", "detail", "sig")

    def __init__(self, kind, detail, sig=""):
        self.kind, self.detail, self.sig = kind, detail, sig

    def key(self):
        return (self.kind, self.sig)


def signature(kind, detail):
    """A coarse key for 'the same bug': minimization keeps it, and only a few failures per key
    are minimized in one run."""
    if kind == "memory":
        return re.sub(r"\d+", "N", detail.split(":", 1)[-1].strip())[:80]
    if kind.startswith("crash-") or kind == "gen-typeerror":
        d = re.sub(r"\d+", "N", detail)
        d = re.sub(r"`[^`]*`", "`X`", d)
        return d[:120]
    return ""


def classify(results):
    fails = []

    def add(kind, detail):
        fails.append(Failure(kind, detail, signature(kind, detail)))

    for r in results.values():
        if r.build_rc != 0:
            kind, detail = build_failure(r)
            add(kind, f"{r.label}: {detail}")
    if fails:
        return fails
    bad = set()
    for key, r in results.items():
        if r.rc is None:
            bad.add(key)
        elif r.target == "c" and r.rc == 102:
            add("memory", f"{r.label}: {err_text(r).strip().splitlines()[-1] if r.err.strip() else 'exit 102'}")
            bad.add(key)
        elif r.target == "c" and r.rc not in (0, 101):
            add("crash-native", f"{r.label}: exit code {r.rc:#x}")
            bad.add(key)
        elif r.target == "js" and r.rc not in (0, 101):
            add("crash-js", f"{r.label}: {js_error(r)}")
            bad.add(key)
    timeouts = [k for k, r in results.items() if r.rc is None]
    if timeouts:
        if len(timeouts) == len(results):
            add("timeout-both", "every build ran too long")
        else:
            for k in timeouts:
                add("timeout-" + ("native" if k[0] == "c" else "js"), f"{results[k].label} ran too long")

    def compare(ka, kb, prefix):
        if ka not in results or kb not in results or ka in bad or kb in bad:
            return
        a, b = results[ka], results[kb]
        if a.rc != b.rc:
            add(prefix + "exit", f"{a.label} exit {a.rc}, {b.label} exit {b.rc}; "
                                 f"stderr: {err_text(a).strip()[:200]!r} / {err_text(b).strip()[:200]!r}")
        elif out_text(a) != out_text(b):
            line, x, y = first_diff(a, b)
            add(prefix + "stdout", f"line {line}: {a.label} {x!r}, {b.label} {y!r}")
        elif a.rc == 101 and err_head(a) != err_head(b):
            add(prefix + "stderr", f"{a.label} {err_head(a)!r}, {b.label} {err_head(b)!r}")

    compare(("c", True), ("js", True), "diff-")
    compare(("c", False), ("js", False), "diff-")
    compare(("c", True), ("c", False), "opt-native-")
    compare(("js", True), ("js", False), "opt-js-")
    # one report per kind
    seen, out = set(), []
    for f in fails:
        if f.key() not in seen:
            seen.add(f.key())
            out.append(f)
    return out


def oracle(src_text, workdir, keys=(("c", True), ("js", True)), name="prog.nyra"):
    os.makedirs(workdir, exist_ok=True)
    with open(os.path.join(workdir, name), "w", encoding="utf-8", newline="\n") as fh:
        fh.write(src_text)
    results = {}
    for target, opt in keys:
        results[(target, opt)] = build_and_run(name, target, opt, workdir)
        if results[(target, opt)].build_rc != 0 and target == "js":
            break  # the front end failed: the other builds fail the same way
    return classify(results), results


def keys_for(kind, detail):
    """The builds needed to see a failure of this kind again (fewer builds = faster shrinking)."""
    nat0 = "NYRA_OPT=0" in detail
    if kind in ("memory", "crash-native", "timeout-native"):
        return (("c", not nat0),)
    if kind in ("crash-js", "timeout-js"):
        return (("js", "NYRA_OPT=0" not in detail),)
    if kind == "crash-cc":
        return (("c", not nat0),)
    if kind.startswith("crash-") or kind == "gen-typeerror":
        return (("js", "NYRA_OPT=0" not in detail),)
    if kind.startswith("opt-native"):
        return (("c", True), ("c", False))
    if kind.startswith("opt-js"):
        return (("js", True), ("js", False))
    if nat0:
        return (("c", False), ("js", False))
    return (("c", True), ("js", True))


# ---------------------------------------------------------------------------------------------
# The minimizer
# ---------------------------------------------------------------------------------------------

class Node:
    __slots__ = ("start", "end", "kids")

    def __init__(self, start, end):
        self.start, self.end, self.kids = start, end, []


def tree(lines):
    """Statements and blocks of a program: a block runs from a line ending with `{` to its `}`."""
    root = Node(-1, len(lines))
    stack = [root]
    for i, l in enumerate(lines):
        s = l.strip()
        if not s or s.startswith("//"):
            continue
        if s.startswith("}") and s.endswith("{"):
            continue  # `} else {`
        if s.endswith("{"):
            n = Node(i, None)
            stack[-1].kids.append(n)
            stack.append(n)
        elif s.startswith("}") and len(stack) > 1:
            stack.pop().end = i + 1
        else:
            stack[-1].kids.append(Node(i, i + 1))
    for n in stack[1:]:
        n.end = len(lines)
    return root


def groups(node):
    """Every list of sibling statements, outermost first."""
    out = [node.kids] if node.kids else []
    for k in node.kids:
        out += groups(k)
    return out


def shrink(lines, ok, deadline):
    """Deletes statements and blocks while ok(lines) holds: chunks of siblings, halving."""
    def try_delete(ranges):
        drop = set()
        for a, b in ranges:
            drop.update(range(a, b))
        cand = [l for i, l in enumerate(lines) if i not in drop]
        return cand if ok(cand) else None

    progress = True
    while progress and time.time() < deadline:
        progress = False
        for sib in groups(tree(lines)):
            n = len(sib)
            chunk = max(1, n // 2)
            while chunk >= 1 and not progress and time.time() < deadline:
                for i in range(0, n, chunk):
                    cand = try_delete([(k.start, k.end) for k in sib[i:i + chunk]])
                    if cand is not None:
                        lines = cand
                        progress = True
                        break
                chunk = chunk // 2 if chunk > 1 else 0
            if progress:
                break
    return lines


def unwrap(lines, ok, deadline):
    """Replaces a block (`if`, `arena`, a loop) by its first part's body."""
    progress = True
    while progress and time.time() < deadline:
        progress = False
        blocks = []

        def walk(n):
            for k in n.kids:
                if k.end - k.start > 1:
                    blocks.append(k)
                walk(k)

        walk(tree(lines))
        for b in blocks:
            head = lines[b.start].strip()
            if head.startswith(("fn ", "struct ")):
                continue
            body, depth = [], 0
            for l in lines[b.start + 1:b.end - 1]:
                s = l.strip()
                if depth == 0 and s.startswith("}") and s.endswith("{"):
                    break
                if s.endswith("{") and not s.startswith("}"):
                    depth += 1
                elif s.startswith("}") and not s.endswith("{"):
                    depth -= 1
                body.append(l)
            cand = lines[:b.start] + body + lines[b.end:]
            if ok(cand):
                lines = cand
                progress = True
                break
    return lines


def simplify_lines(lines, ok, deadline):
    """Small rewrites of single lines: drop an `else` part, replace an argument of print."""
    i = 0
    while i < len(lines) and time.time() < deadline:
        s = lines[i].strip()
        if s == "} else {":
            # drop the else part
            depth, j = 0, i + 1
            while j < len(lines):
                t = lines[j].strip()
                if t.endswith("{") and not t.startswith("}"):
                    depth += 1
                elif t.startswith("}") and not t.endswith("{"):
                    if depth == 0:
                        break
                    depth -= 1
                j += 1
            cand = lines[:i] + lines[j:]
            if ok(cand):
                lines = cand
                continue
        i += 1
    return lines


TOKEN = re.compile(r"""
    "(?:[^"\\{]|\\.|\{\{|\{[^{}"]*(?:\{[^{}"]*\}[^{}"]*)*\})*"   # string (with `{...}` parts)
  | '(?:[^'\\]|\\.)'                                             # char
  | \d+(?:\.\d+)?                                                # number
  | \w+                                                          # name
  | \.\.|==|!=|<=|>=|&&|\|\||[-+*/%]=|->                         # two-character operators
  | \S                                                           # anything else
""", re.X)


def tokens(line):
    return [(m.group(0), m.start(), m.end()) for m in TOKEN.finditer(line)]


def matching(toks, i):
    """The index of the bracket that closes toks[i], or None."""
    pairs = {"(": ")", "[": "]", "{": "}"}
    open_, close = toks[i][0], pairs[toks[i][0]]
    depth = 0
    for j in range(i, len(toks)):
        if toks[j][0] == open_:
            depth += 1
        elif toks[j][0] == close:
            depth -= 1
            if depth == 0:
                return j
    return None


def top_commas(toks, a, b):
    """Positions of the commas directly inside toks[a] .. toks[b]."""
    out, depth = [], 0
    for k in range(a + 1, b):
        t = toks[k][0]
        if t in "([{":
            depth += 1
        elif t in ")]}":
            depth -= 1
        elif t == "," and depth == 0:
            out.append(k)
    return out


def line_rewrites(line):
    """Smaller versions of one line: an if-value replaced by a branch, parentheses dropped,
    an array literal with fewer elements, shorter literals, a method call removed."""
    toks = tokens(line)
    out = []

    def span(i, j, new):
        out.append(line[:toks[i][1]] + new + line[toks[j][2]:])

    for i, (t, a, b) in enumerate(toks):
        if t == "if" and i > 0:
            k = i + 1
            while k < len(toks) and toks[k][0] not in ("{", "if"):
                k += 1
            if k < len(toks) and toks[k][0] == "{":
                j = matching(toks, k)
                if j is not None and j + 2 < len(toks) and toks[j + 1][0] == "else" and toks[j + 2][0] == "{":
                    e = matching(toks, j + 2)
                    if e is not None:
                        span(i, e, line[toks[k][2]:toks[j][1]].strip())
                        span(i, e, line[toks[j + 2][2]:toks[e][1]].strip())
        elif t == "(":
            j = matching(toks, i)
            if j is not None and (i == 0 or not re.match(r"\w", toks[i - 1][0])):
                span(i, j, line[b:toks[j][1]])
        elif t == "[" and (i == 0 or not re.match(r"[\w)\]]", toks[i - 1][0])):
            j = matching(toks, i)
            if j is not None:
                commas = top_commas(toks, i, j)
                if commas:
                    span(i, j, "[" + line[b:toks[commas[0]][1]].strip() + "]")
                    span(i, j, "[" + line[toks[commas[-1]][2]:toks[j][1]].strip() + "]")
        elif t.startswith('"') and t not in ('""', '"a"'):
            span(i, i, '""')
            span(i, i, '"a"')
        elif re.fullmatch(r"\d+", t) and t not in ("0", "1"):
            span(i, i, "0")
            span(i, i, "1")
        elif re.fullmatch(r"\d+\.\d+", t) and t not in ("0.0", "1.0"):
            span(i, i, "1.0")
        elif t == "." and i + 2 < len(toks) and toks[i + 2][0] == "(":
            j = matching(toks, i + 2)
            if j is not None:
                span(i, j, "")
    return out


def simplify_exprs(lines, ok, deadline):
    """Rewrites expressions inside lines (see line_rewrites) while the failure stays."""
    progress = True
    while progress and time.time() < deadline:
        progress = False
        for i in range(len(lines)):
            s = lines[i].strip()
            if s.startswith(("fn ", "struct ", "//")) and not s.startswith("fn main"):
                continue
            for cand in line_rewrites(lines[i]):
                if time.time() >= deadline:
                    break
                if cand != lines[i] and ok(lines[:i] + [cand] + lines[i + 1:]):
                    lines = lines[:i] + [cand] + lines[i + 1:]
                    progress = True
                    break
    return lines


def tidy(lines):
    """No blank lines, except one before each top-level item."""
    out = []
    for l in lines:
        if not l.strip():
            continue
        if out and not l.startswith((" ", "}")) and l.startswith(("fn ", "struct ")):
            out.append("")
        out.append(l)
    return out


def minimize(src_text, failure, workdir, budget_s=1200, log=None):
    keys = keys_for(failure.kind, failure.detail)
    cache = {}
    calls = [0]

    def ok(lines):
        src = "\n".join(lines).rstrip("\n") + "\n"
        h = hashlib.sha1(src.encode("utf-8")).hexdigest()
        if h not in cache:
            calls[0] += 1
            fails, _ = oracle(src, workdir, keys, name="min.nyra")
            cache[h] = any(f.key() == failure.key() for f in fails)
        return cache[h]

    lines = src_text.rstrip("\n").split("\n")
    if not ok(lines):
        if log:
            log(f"  could not reproduce {failure.kind} with {keys}")
        return None
    deadline = time.time() + budget_s
    while True:
        before = len("".join(lines))
        lines = shrink(lines, ok, deadline)
        lines = unwrap(lines, ok, deadline)
        lines = simplify_lines(lines, ok, deadline)
        lines = shrink(lines, ok, deadline)
        lines = simplify_exprs(lines, ok, deadline)
        if len("".join(lines)) >= before or time.time() >= deadline:
            break
    t = tidy(lines)
    if ok(t):
        lines = t
    if log:
        log(f"  minimized {failure.kind} to {len(lines)} lines in {calls[0]} runs")
    return "\n".join(lines).rstrip("\n") + "\n"


def describe(src_text, failure, workdir):
    """The header comment of a minimized failure: what each backend did."""
    keys = keys_for(failure.kind, failure.detail)
    _, results = oracle(src_text, workdir, keys, name="desc.nyra")
    out = []
    for r in results.values():
        if r.build_rc != 0:
            out.append(f"{r.label}: build failed: {build_failure(r)[1]}")
            continue
        o = out_text(r).strip().replace("\n", " | ")
        e = err_text(r).strip().split("\n")[0]
        out.append(f"{r.label}: exit {r.rc}, stdout {o[:300]!r}" + (f", stderr {e[:200]!r}" if e else ""))
    return out


def save_failure(src_text, failure, seed, outdir, workdir, log=None):
    os.makedirs(outdir, exist_ok=True)
    path = os.path.join(outdir, f"{failure.kind}-{seed}.nyra")
    lines = [f"// fuzz: {failure.kind} (seed {seed}): {failure.detail[:300]}"]
    try:
        lines += ["// " + d for d in describe(src_text, failure, workdir)]
    except Exception as ex:  # the description is only a help
        lines.append(f"// (no description: {ex})")
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        fh.write("\n".join(lines) + "\n" + src_text)
    if log:
        log(f"  wrote {os.path.relpath(path, ROOT)}")
    return path


# ---------------------------------------------------------------------------------------------
# Running many programs
# ---------------------------------------------------------------------------------------------

def opt0_selected(seed, percent):
    return (seed * 2654435761) % 100 < percent


def parse_features(items):
    feats = {}
    for item in items or []:
        for part in item.split(","):
            if not part:
                continue
            name, _, val = part.partition("=")
            if name not in FEATURES:
                raise SystemExit(f"unknown feature `{name}` (known: {', '.join(FEATURES)})")
            feats[name] = val.lower() not in ("0", "off", "false", "no")
    return feats


class Campaign:
    def __init__(self, args):
        self.args = args
        self.lock = threading.Lock()
        self.count = 0
        self.kinds = {}
        self.minimized = {}
        self.local = threading.local()
        self.workroot = args.workdir or os.path.join(tempfile.gettempdir(), "nyra_fuzz")
        self.rawdir = os.path.join(self.workroot, "raw")
        os.makedirs(self.rawdir, exist_ok=True)
        self.feats = parse_features(args.feature)
        self.start = time.time()
        self.stop = False
        self.logfh = open(os.path.join(self.workroot, "fuzz.log"), "a", encoding="utf-8")

    def log(self, msg):
        with self.lock:
            print(msg, flush=True)
            self.logfh.write(msg + "\n")
            self.logfh.flush()

    def workdir(self):
        if not hasattr(self.local, "dir"):
            with self.lock:
                n = len(os.listdir(self.workroot))
            self.local.dir = tempfile.mkdtemp(prefix="w", dir=self.workroot)
        return self.local.dir

    def one(self, seed):
        if self.stop or (self.args.time_limit and time.time() - self.start > self.args.time_limit):
            self.stop = True
            return
        src = generate(seed, self.feats)
        keys = [("c", True), ("js", True)]
        if opt0_selected(seed, self.args.opt0):
            keys += [("c", False), ("js", False)]
        fails, _ = oracle(src, self.workdir(), keys)
        with self.lock:
            self.count += 1
            for f in fails:
                self.kinds[f.kind] = self.kinds.get(f.kind, 0) + 1
            n = self.count
        for f in fails:
            raw = os.path.join(self.rawdir, f"{f.kind}-{seed}.nyra")
            with open(raw, "w", encoding="utf-8", newline="\n") as fh:
                fh.write(src)
            self.log(f"seed {seed}: {f.kind}: {f.detail[:400]}")
            if self.args.no_minimize or f.kind == "gen-typeerror":
                continue
            with self.lock:
                done = self.minimized.get(f.key(), 0)
                if done >= self.args.minimize_limit:
                    continue
                self.minimized[f.key()] = done + 1
            small = minimize(src, f, os.path.join(self.workdir(), "min"), self.args.minimize_time, self.log)
            if small is not None:
                save_failure(small, f, seed, self.args.out, os.path.join(self.workdir(), "min"), self.log)
        if n % 50 == 0:
            el = time.time() - self.start
            self.log(f"[{n} programs, {el:.0f}s, {n / el:.2f}/s] " +
                     ", ".join(f"{k}: {v}" for k, v in sorted(self.kinds.items())))

    def run(self):
        seeds = range(self.args.seed, self.args.seed + self.args.count)
        try:
            with ThreadPoolExecutor(self.args.jobs) as ex:
                list(ex.map(self.one, seeds))
        except KeyboardInterrupt:
            self.stop = True
        el = time.time() - self.start
        self.log(f"done: {self.count} programs in {el:.0f}s; " +
                 (", ".join(f"{k}: {v}" for k, v in sorted(self.kinds.items())) or "no failures"))


def main():
    ap = argparse.ArgumentParser(description="Differential fuzzer for the Nyra C and JavaScript backends.")
    ap.add_argument("--seed", type=int, default=1, help="first seed")
    ap.add_argument("--count", type=int, default=100, help="how many programs (seeds seed..seed+count-1)")
    ap.add_argument("--jobs", type=int, default=max(1, (os.cpu_count() or 2) - 1))
    ap.add_argument("--opt0", type=int, default=25, help="percent of programs also built with NYRA_OPT=0")
    ap.add_argument("--feature", action="append", help="turn generator features on/off: name=on,name=off")
    ap.add_argument("--out", default=FAILURES, help="where minimized failures go")
    ap.add_argument("--workdir", help="scratch directory (default: <temp>/nyra_fuzz)")
    ap.add_argument("--no-minimize", action="store_true")
    ap.add_argument("--minimize-limit", type=int, default=2, help="failures to minimize per kind and signature")
    ap.add_argument("--minimize-time", type=int, default=900, help="seconds per minimization")
    ap.add_argument("--time-limit", type=int, default=0, help="stop starting programs after this many seconds")
    ap.add_argument("--print", type=int, metavar="SEED", help="print the program of a seed and exit")
    ap.add_argument("--check", metavar="FILE", help="run a file through the oracle")
    ap.add_argument("--minimize", metavar="FILE", help="minimize a failing file")
    ap.add_argument("--kind", help="with --minimize: which failure to keep (default: the first)")
    args = ap.parse_args()

    if sys.stdout.encoding and sys.stdout.encoding.lower() not in ("utf-8", "utf8"):
        sys.stdout.reconfigure(encoding="utf-8")
    if not os.path.exists(NYRA):
        raise SystemExit(f"{NYRA} not found: run `cargo build --release` first")
    feats = parse_features(args.feature)
    if args.print is not None:
        sys.stdout.write(generate(args.print, feats))
        return
    if args.check or args.minimize:
        path = args.check or args.minimize
        with open(path, encoding="utf-8") as fh:
            src = fh.read()
        work = tempfile.mkdtemp(prefix="nyra_fuzz_")
        fails, results = oracle(src, work, (("c", True), ("js", True), ("c", False), ("js", False)))
        for r in results.values():
            print(f"{r.label}: build {r.build_rc}, exit {r.rc}")
        for f in fails:
            print(f"{f.kind}: {f.detail}")
        if not fails:
            print("no failure")
        if args.minimize and fails:
            f = next((x for x in fails if x.kind == args.kind), fails[0])
            small = minimize(src, f, os.path.join(work, "min"), args.minimize_time, print)
            if small is not None:
                m = re.search(r"-(\d+)\.nyra$", path)
                save_failure(small, f, m.group(1) if m else "file", args.out, os.path.join(work, "min"), print)
        shutil.rmtree(work, ignore_errors=True)
        return
    Campaign(args).run()


if __name__ == "__main__":
    main()
