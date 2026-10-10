

# ---- standard library: `use input`, `use os`, `use fs`, `use time`, `use random`, `use math`, `use text`

import errno as ny_errno
import time as ny_time


def ny_fs_fail(what, path, reason, line, col):
    ny_panic("E0340", f"{what} \"{ny_shown(path)}\" ({reason})",
             "check the path: it is relative to the folder the program runs in (`fs.exists(path)` tests first)", line, col)


def ny_errno_reason(e):
    if isinstance(e, (FileNotFoundError, NotADirectoryError)):
        return "not found"
    if isinstance(e, PermissionError):
        return "permission denied"
    if isinstance(e, FileExistsError):
        return "already exists"
    if getattr(e, "errno", None) == ny_errno.ENOTEMPTY:
        return "not empty"
    return "io error"


def ny_text(b):
    """Bytes as text, or None when they are not valid UTF-8."""
    try:
        return b.decode("utf-8")
    except UnicodeDecodeError:
        return None


def ny_valid(s):
    """False for a str that came from bytes that are not UTF-8 (Python keeps them as surrogates)."""
    try:
        s.encode("utf-8")
        return True
    except UnicodeEncodeError:
        return False


def ny_kind(path):
    """0: nothing there (also for "" and a path with a NUL), 1: a file, 2: a directory."""
    if path == "" or "\0" in path:
        return 0
    if os.path.isdir(path):
        return 2
    return 1 if os.path.exists(path) else 0


# ---- fs ----

def ny_std_fs_read(path, line, col):
    what = "fs.read: cannot read"
    k = ny_kind(path)
    if k == 0:
        ny_fs_fail(what, path, "not found", line, col)
    if k == 2:
        ny_fs_fail(what, path, "is a directory", line, col)
    try:
        with open(path, "rb") as f:
            data = f.read()
    except OSError as e:
        ny_fs_fail(what, path, ny_errno_reason(e), line, col)
    s = ny_text(data)
    if s is None:
        ny_fs_fail(what, path, "not valid UTF-8", line, col)
    return s


def ny_fs_put(path, text, mode, what, line, col):
    if path == "" or "\0" in path:
        ny_fs_fail(what, path, "not found", line, col)
    if ny_kind(path) == 2:
        ny_fs_fail(what, path, "is a directory", line, col)
    try:
        with open(path, mode) as f:
            f.write(text.encode("utf-8"))
    except OSError as e:
        ny_fs_fail(what, path, ny_errno_reason(e), line, col)


def ny_std_fs_write(path, text, line, col):
    ny_fs_put(path, text, "wb", "fs.write: cannot write", line, col)


def ny_std_fs_append(path, text, line, col):
    ny_fs_put(path, text, "ab", "fs.append: cannot append to", line, col)


def ny_std_fs_exists(path, line, col):
    return ny_kind(path) != 0


def ny_std_fs_list(d, line, col):
    what = "fs.list: cannot list"
    k = ny_kind(d)
    if k == 0:
        ny_fs_fail(what, d, "not found", line, col)
    if k == 1:
        ny_fs_fail(what, d, "not a directory", line, col)
    try:
        names = os.listdir(d)
    except OSError as e:
        ny_fs_fail(what, d, ny_errno_reason(e), line, col)
    for n in names:
        if not ny_valid(n):
            ny_fs_fail(what, d, "not valid UTF-8", line, col)
    return NyList(sorted(names))


def ny_std_fs_remove(path, line, col):
    what = "fs.remove: cannot remove"
    k = ny_kind(path)
    if k == 0:
        ny_fs_fail(what, path, "not found", line, col)
    if k == 2 and len(ny_std_fs_list(path, line, col)) > 0:
        ny_fs_fail(what, path, "not empty", line, col)
    try:
        if k == 2:
            os.rmdir(path)
        else:
            os.remove(path)
    except OSError as e:
        ny_fs_fail(what, path, ny_errno_reason(e), line, col)


def ny_std_fs_mkdir(path, line, col):
    what = "fs.mkdir: cannot create"
    if path == "" or "\0" in path:
        ny_fs_fail(what, path, "not found", line, col)
    if ny_kind(path) != 0:
        ny_fs_fail(what, path, "already exists", line, col)
    try:
        os.mkdir(path)
    except OSError as e:
        ny_fs_fail(what, path, ny_errno_reason(e), line, col)


# ---- input: standard input as bytes (the same on every system) ----

def ny_in_text(b, what, line, col):
    s = ny_text(b)
    if s is None:
        ny_panic("E0341", f"{what}: the input is not valid UTF-8", "standard input must be UTF-8 text", line, col)
    return s


def ny_stdin():
    return sys.stdin.buffer if sys.stdin is not None else None


def ny_std_input_line(line, col):
    sys.stdout.flush()
    f = ny_stdin()
    b = f.readline() if f is not None else b""
    if b.endswith(b"\n"):
        b = b[:-1]
        if b.endswith(b"\r"):
            b = b[:-1]
    return ny_in_text(b, "input.line", line, col)


def ny_std_input_eof(line, col):
    sys.stdout.flush()
    f = ny_stdin()
    return f is None or len(f.peek(1)) == 0


def ny_std_input_all(line, col):
    sys.stdout.flush()
    f = ny_stdin()
    return ny_in_text(f.read() if f is not None else b"", "input.all", line, col)


def ny_lines(s):
    """The lines of a text: each without its `\\n` (and a `\\r` before it); no last empty line."""
    out = NyList()
    i = 0
    while i < len(s):
        nl = s.find("\n", i)
        end = len(s) if nl < 0 else nl
        nxt = len(s) if nl < 0 else nl + 1
        if nl >= 0 and end > i and s[end - 1] == "\r":
            end -= 1
        out.append(s[i:end])
        i = nxt
    return out


def ny_std_input_lines(line, col):
    sys.stdout.flush()
    f = ny_stdin()
    return ny_lines(ny_in_text(f.read() if f is not None else b"", "input.lines", line, col))


# ---- os ----

def ny_std_os_args(line, col):
    args = sys.argv[1:]
    for a in args:
        if not ny_valid(a):
            ny_panic("E0341", "os.args: an argument is not valid UTF-8", "program arguments must be UTF-8 text", line, col)
    return NyList(args)


def ny_getenv(name, line, col):
    if name == "" or "=" in name or "\0" in name:
        return None
    v = os.environ.get(name)
    if v is not None and not ny_valid(v):
        ny_panic("E0341", "os.env: the value is not valid UTF-8", "environment variables must be UTF-8 text", line, col)
    return v


def ny_std_os_env(name, line, col):
    v = ny_getenv(name, line, col)
    return "" if v is None else v


def ny_std_os_has_env(name, line, col):
    return ny_getenv(name, line, col) is not None


def ny_std_os_exit(code, line, col):
    sys.stdout.flush()
    sys.exit(code & 255)


# ---- time ----

def ny_std_time_now_ms(line, col):
    return ny_time.time_ns() // 1000000


def ny_std_time_mono_ms(line, col):
    return ny_time.perf_counter_ns() / 1e6


def ny_std_time_sleep_ms(ms, line, col):
    sys.stdout.flush()
    if ms > 0:
        ny_time.sleep(ms / 1000)


# ---- random: the operating system's generator, or after `random.seed(n)` xoshiro128** ----

NY_M32 = 0xFFFFFFFF
ny_seeded = False
ny_rs = [0, 0, 0, 0]
ny_pool = []


def ny_rotl(x, k):
    return ((x << k) | (x >> (32 - k))) & NY_M32


def ny_mix32(z):
    z = ((z ^ (z >> 16)) * 0x85EBCA6B) & NY_M32
    z = ((z ^ (z >> 13)) * 0xC2B2AE35) & NY_M32
    return z ^ (z >> 16)


def ny_bits32():
    if ny_seeded:
        s = ny_rs
        r = (ny_rotl((s[1] * 5) & NY_M32, 7) * 9) & NY_M32
        t = (s[1] << 9) & NY_M32
        s[2] ^= s[0]
        s[3] ^= s[1]
        s[1] ^= s[2]
        s[0] ^= s[3]
        s[2] ^= t
        s[3] = ny_rotl(s[3], 11)
        return r
    if not ny_pool:
        b = os.urandom(256)
        ny_pool.extend(int.from_bytes(b[i:i + 4], "little") for i in range(0, 256, 4))
    return ny_pool.pop()


def ny_std_random_seed(n, line, col):
    global ny_seeded
    half = (n & NY_M32, (n >> 32) & NY_M32)
    for i in range(4):
        ny_rs[i] = ny_mix32((half[i & 1] + (i + 1) * 0x9E3779B9) & NY_M32)
    if not any(ny_rs):
        ny_rs[0] = 1
    ny_seeded = True


def ny_bits53():
    """53 random bits: the first draw gives the high 27, the second the low 26."""
    a = ny_bits32() >> 5
    b = ny_bits32() >> 6
    return a * 67108864 + b


def ny_std_random_random(line, col):
    return ny_bits53() / 9007199254740992.0


def ny_std_random_range(lo, hi, line, col):
    if hi <= lo or hi - lo > 9007199254740992:
        ny_panic("E0342", f"random.range({lo}, {hi}): need lo < hi and hi - lo <= 2^53",
                 "the upper bound is excluded: `random.range(1, 7)` rolls a die", line, col)
    n = hi - lo
    limit = 9007199254740992 - 9007199254740992 % n
    r = ny_bits53()
    while r >= limit:
        r = ny_bits53()
    return lo + r % n


# ---- math: operations that are exact on every host (the sign of a zero result is kept) ----

def ny_std_math_sqrt(x, line, col):
    return math.sqrt(x) if x >= 0 else math.nan


def ny_whole(r, x):
    r = float(r)
    return math.copysign(r, x) if r == 0 else r


def ny_std_math_floor(x, line, col):
    return ny_whole(math.floor(x), x) if math.isfinite(x) else x


def ny_std_math_ceil(x, line, col):
    return ny_whole(math.ceil(x), x) if math.isfinite(x) else x


def ny_std_math_trunc(x, line, col):
    return ny_whole(math.trunc(x), x) if math.isfinite(x) else x


def ny_std_math_round(x, line, col):
    """Half away from zero (x - trunc(x) is exact)."""
    t = ny_std_math_trunc(x, line, col)
    if abs(x - t) >= 0.5:
        t += -1.0 if x < 0 else 1.0
    return t


# ---- text ----

def ny_std_text_is_int(s, line, col):
    if not NY_INT.fullmatch(s) or len(s.lstrip("-").lstrip("0")) > 19:
        return False
    return -9223372036854775808 <= int(s) <= 9223372036854775807


def ny_std_text_is_float(s, line, col):
    return NY_FLOAT.fullmatch(s) is not None


def ny_std_text_fixed(x, d, line, col):
    """The exact value of x rounded to d decimals, ties away from zero."""
    if d < 0 or d > 100:
        ny_panic("E0342", f"text.fixed: digits must be 0 to 100, got {d}", "`text.fixed(x, 2)` shows two decimals", line, col)
    if not math.isfinite(x):
        return ny_num(x)
    frac, e = math.frexp(abs(x))
    m = int(frac * 9007199254740992)
    s = e - 53 + d   # x * 10^d = m * 5^d * 2^s
    a = m * 5 ** d
    q = a << s if s >= 0 else (a >> -s) + ((a >> (-s - 1)) & 1)
    digits = str(q)
    if d > 0:
        digits = digits.rjust(d + 1, "0")
        digits = digits[:-d] + "." + digits[-d:]
    return ("-" if math.copysign(1.0, x) < 0 and q != 0 else "") + digits
