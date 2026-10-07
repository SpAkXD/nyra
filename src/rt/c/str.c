// ---- memory: every heap object goes through nyrt_alloc / nyrt_free -----------------
// NYRA_LEAKCHECK=1 turns on the memory check (for nyra's own tests): a live-object counter
// catches leaks at exit, and freed objects stay allocated as tombstones (rc -1), so a second
// release or a use after free stops the program with exit code 102 instead of corrupting memory.
static int64_t nyrt_live = 0;
static bool nyrt_checking = false;
static void nyrt_init(void) { nyrt_checking = getenv("NYRA_LEAKCHECK") != NULL; }
static void nyrt_bug(const char *what) {
    fflush(stdout);
    fprintf(stderr, "nyra: memory check failed: %s (this is a nyra bug)\n", what);
    exit(102);
}
// Out of memory. The position is the operation that asked for the memory, when it is known.
static void nyrt_oom(int line, int col) {
    nyrt_panic("E0249", "out of memory", "the program needs more memory than the system gave it", line, col);
}
static void *nyrt_alloc_at(size_t n, int line, int col) {
    void *p = malloc(n);
    if (!p) nyrt_oom(line, col);
    nyrt_live++;
    return p;
}
static void *nyrt_alloc(size_t n) { return nyrt_alloc_at(n, 0, 0); }
static void *nyrt_realloc(void *p, size_t n) {
    void *q = realloc(p, n);
    if (!q) nyrt_oom(0, 0);
    return q;
}
static void nyrt_free(void *p) {
    nyrt_live--;
    free(p);
}
static void nyrt_leak_check(void) {
    fflush(stdout);
    if (getenv("NYRA_LEAKCHECK") && nyrt_live != 0) {
        fprintf(stderr, "nyra: leak check failed: %lld objects alive at exit (this is a nyra bug)\n", (long long)nyrt_live);
        exit(102);
    }
}

// ---- strings: reference counted, UTF-8, lengths in characters (code points) -----------
// rc == 0 means immortal (literals): retain/release do nothing, writes copy first.
typedef uint32_t nyrt_char;
typedef struct nyrt_str { int64_t rc, len, cap, nchars; char *data; } nyrt_str;   // len/cap in bytes

// The longest string JavaScript engines allow; repeat() stops at it on every backend.
#define NYRT_MAX_STR 536870888
static nyrt_str *nyrt_str_alloc_at(int64_t cap, int line, int col) {
    nyrt_str *s = nyrt_alloc_at(sizeof(nyrt_str) + (size_t)cap + 1, line, col);
    s->rc = 1; s->len = 0; s->cap = cap; s->nchars = 0;
    s->data = (char *)(s + 1);
    s->data[0] = '\0';
    return s;
}
static nyrt_str *nyrt_str_alloc(int64_t cap) {
    nyrt_str *s = nyrt_alloc(sizeof(nyrt_str) + (size_t)cap + 1);
    s->rc = 1; s->len = 0; s->cap = cap; s->nchars = 0;
    s->data = (char *)(s + 1);
    s->data[0] = '\0';
    return s;
}
static void nyrt_str_retain(nyrt_str *s) {
    if (!s || !s->rc) return;
    if (s->rc < 0) nyrt_bug("a freed string was used again");
    s->rc++;
}
// `s[i]` on non-ASCII text: the last (character, byte) position found, so a loop over the
// indexes walks the string once instead of once per index. Dropped when its string changes or dies.
static const nyrt_str *nyrt_pos_s = NULL;
static int64_t nyrt_pos_ci = 0, nyrt_pos_bi = 0;
static void nyrt_str_release(nyrt_str *s) {
    if (!s || !s->rc) return;
    if (s->rc < 0) nyrt_bug("a string was freed twice");
    if (--s->rc > 0) return;
    if (s == nyrt_pos_s) nyrt_pos_s = NULL;
    if (nyrt_checking) { s->rc = -1; nyrt_live--; }   // tombstone
    else nyrt_free(s);
}
// `keep(s)`: never freed from now on (reference count 0, like a literal), not a leak.
static void nyrt_str_keep(nyrt_str *s) {
    if (!s || s->rc <= 0) return;
    s->rc = 0;
    nyrt_live--;
}
// Under the memory check, every operation first makes sure its strings are alive.
#define NYRT_LIVE(s) do { if (nyrt_checking && (s)->rc < 0) nyrt_bug("a freed string was used again"); } while (0)

static int64_t nyrt_utf8_count(const char *p, int64_t n) {
    int64_t c = 0;
    for (int64_t i = 0; i < n; i++) if (((unsigned char)p[i] & 0xC0) != 0x80) c++;
    return c;
}
static nyrt_str *nyrt_str_from(const char *p, int64_t n) {
    nyrt_str *s = nyrt_str_alloc(n);
    memcpy(s->data, p, (size_t)n);
    s->data[n] = '\0';
    s->len = n;
    s->nchars = nyrt_utf8_count(p, n);
    return s;
}
// Byte offset of character `i` (0 <= i <= nchars).
static int64_t nyrt_str_offset(const nyrt_str *s, int64_t i) {
    NYRT_LIVE(s);
    if (s->nchars == s->len) return i;   // ASCII: O(1)
    int64_t ci = 0, b = 0;
    if (s == nyrt_pos_s && i >= nyrt_pos_ci) {
        ci = nyrt_pos_ci;   // continue from the last position
        b = nyrt_pos_bi;
    } else if (s == nyrt_pos_s && nyrt_pos_ci - i < i) {
        ci = nyrt_pos_ci;   // closer to the last position than to the start: walk back
        b = nyrt_pos_bi;
        while (ci > i) {
            b--;
            while (b > 0 && ((unsigned char)s->data[b] & 0xC0) == 0x80) b--;
            ci--;
        }
    }
    while (ci < i && b < s->len) {
        b++;
        while (b < s->len && ((unsigned char)s->data[b] & 0xC0) == 0x80) b++;
        ci++;
    }
    nyrt_pos_s = s;
    nyrt_pos_ci = ci;
    nyrt_pos_bi = b;
    return b;
}
static nyrt_char nyrt_utf8_decode(const char *p, int64_t *adv) {
    const unsigned char *u = (const unsigned char *)p;
    if (u[0] < 0x80) { *adv = 1; return u[0]; }
    if (u[0] < 0xE0) { *adv = 2; return ((nyrt_char)(u[0] & 0x1F) << 6) | (u[1] & 0x3F); }
    if (u[0] < 0xF0) { *adv = 3; return ((nyrt_char)(u[0] & 0x0F) << 12) | ((nyrt_char)(u[1] & 0x3F) << 6) | (u[2] & 0x3F); }
    *adv = 4;
    return ((nyrt_char)(u[0] & 0x07) << 18) | ((nyrt_char)(u[1] & 0x3F) << 12) | ((nyrt_char)(u[2] & 0x3F) << 6) | (u[3] & 0x3F);
}
static int nyrt_utf8_encode(nyrt_char c, char *out) {
    if (c < 0x80) { out[0] = (char)c; return 1; }
    if (c < 0x800) { out[0] = (char)(0xC0 | (c >> 6)); out[1] = (char)(0x80 | (c & 0x3F)); return 2; }
    if (c < 0x10000) {
        out[0] = (char)(0xE0 | (c >> 12)); out[1] = (char)(0x80 | ((c >> 6) & 0x3F)); out[2] = (char)(0x80 | (c & 0x3F));
        return 3;
    }
    out[0] = (char)(0xF0 | (c >> 18)); out[1] = (char)(0x80 | ((c >> 12) & 0x3F));
    out[2] = (char)(0x80 | ((c >> 6) & 0x3F)); out[3] = (char)(0x80 | (c & 0x3F));
    return 4;
}

// ---- building strings: interpolation, str(x), +, join, ... -------------------------
typedef struct nyrt_buf { nyrt_str *s; } nyrt_buf;
static nyrt_buf nyrt_buf_new(void) { nyrt_buf b = { nyrt_str_alloc(16) }; return b; }
static void nyrt_buf_add(nyrt_buf *b, const char *p, int64_t n) {
    nyrt_str *s = b->s;
    if (s == nyrt_pos_s) nyrt_pos_s = NULL;   // it changes (and may move)
    if (s->len + n > s->cap) {
        int64_t cap = s->cap * 2;
        if (cap < s->len + n) cap = s->len + n;
        s = nyrt_realloc(s, sizeof(nyrt_str) + (size_t)cap + 1);
        s->data = (char *)(s + 1);
        s->cap = cap;
        b->s = s;
    }
    memcpy(s->data + s->len, p, (size_t)n);
    s->len += n;
    s->data[s->len] = '\0';
    s->nchars += nyrt_utf8_count(p, n);
}
static void nyrt_buf_str(nyrt_buf *b, const nyrt_str *s) { NYRT_LIVE(s); nyrt_buf_add(b, s->data, s->len); }
static void nyrt_buf_cstr(nyrt_buf *b, const char *p) { nyrt_buf_add(b, p, (int64_t)strlen(p)); }
static void nyrt_buf_int(nyrt_buf *b, int64_t x) { char t[24]; nyrt_buf_add(b, t, snprintf(t, sizeof t, "%lld", (long long)x)); }
static void nyrt_buf_float(nyrt_buf *b, double x) { char t[32]; nyrt_buf_cstr(b, nyrt_float_fmt(t, x)); }
static void nyrt_buf_bool(nyrt_buf *b, bool x) { nyrt_buf_cstr(b, x ? "true" : "false"); }
static void nyrt_buf_char(nyrt_buf *b, nyrt_char c) { char t[4]; nyrt_buf_add(b, t, nyrt_utf8_encode(c, t)); }
static void nyrt_buf_lit(nyrt_buf *b, const char *p, int64_t n) { nyrt_buf_add(b, p, n); }
static nyrt_str *nyrt_buf_done(nyrt_buf *b) { return b->s; }

// ---- printing: each part of a print goes straight to stdout --------------------------
static void nyrt_put_lit(const char *p, int64_t n) { fwrite(p, 1, (size_t)n, stdout); }
static void nyrt_put_str(const nyrt_str *s) { NYRT_LIVE(s); fwrite(s->data, 1, (size_t)s->len, stdout); }
static void nyrt_put_int(int64_t x) { printf("%lld", (long long)x); }
static void nyrt_put_float(double x) { char t[32]; fputs(nyrt_float_fmt(t, x), stdout); }
static void nyrt_put_bool(bool x) { fputs(x ? "true" : "false", stdout); }
static void nyrt_put_char(nyrt_char c) { char t[4]; fwrite(t, 1, (size_t)nyrt_utf8_encode(c, t), stdout); }

// ---- string operations -------------------------------------------------------------
static bool nyrt_str_eq(const nyrt_str *a, const nyrt_str *b) {
    NYRT_LIVE(a); NYRT_LIVE(b);
    return a == b || (a->len == b->len && memcmp(a->data, b->data, (size_t)a->len) == 0);
}
// Code point order (= UTF-8 byte order).
static int nyrt_str_cmp(const nyrt_str *a, const nyrt_str *b) {
    NYRT_LIVE(a); NYRT_LIVE(b);
    int64_t n = a->len < b->len ? a->len : b->len;
    int c = memcmp(a->data, b->data, (size_t)n);
    if (c) return c < 0 ? -1 : 1;
    return a->len < b->len ? -1 : a->len > b->len ? 1 : 0;
}
static nyrt_str *nyrt_str_concat(const nyrt_str *a, const nyrt_str *b) {
    NYRT_LIVE(a); NYRT_LIVE(b);
    nyrt_str *s = nyrt_str_alloc(a->len + b->len);
    memcpy(s->data, a->data, (size_t)a->len);
    memcpy(s->data + a->len, b->data, (size_t)b->len);
    s->len = a->len + b->len;
    s->data[s->len] = '\0';
    s->nchars = a->nchars + b->nchars;
    return s;
}
// `a += b`: in place when `a` is the only owner, otherwise a new string.
static void nyrt_str_append(nyrt_str **a, const nyrt_str *b) {
    nyrt_str *s = *a;
    NYRT_LIVE(s); NYRT_LIVE(b);
    if (s->rc != 1) {
        *a = nyrt_str_concat(s, b);
        nyrt_str_release(s);
        return;
    }
    nyrt_buf buf = { s };
    nyrt_buf_add(&buf, b->data, b->len);
    *a = buf.s;
}
static nyrt_char nyrt_str_at(const nyrt_str *s, int64_t i, int line, int col) {
    if (i < 0 || i >= s->nchars) {
        char msg[96];
        snprintf(msg, sizeof msg, "index %lld is out of bounds for length %lld", (long long)i, (long long)s->nchars);
        nyrt_panic("E0240", msg, "valid indexes are 0 to len - 1; compare with `.len()` first", line, col);
    }
    int64_t adv;
    return nyrt_utf8_decode(s->data + nyrt_str_offset(s, i), &adv);
}
static void nyrt_range_check(int64_t a, int64_t b, int64_t n, int line, int col) {
    if (a < 0 || a > b || b > n) {
        char msg[128];
        snprintf(msg, sizeof msg, "range %lld..%lld is out of bounds for length %lld", (long long)a, (long long)b, (long long)n);
        nyrt_panic("E0240", msg, "a range a..b needs 0 <= a <= b <= len", line, col);
    }
}
static nyrt_str *nyrt_str_slice(const nyrt_str *s, int64_t a, int64_t b, int line, int col) {
    nyrt_range_check(a, b, s->nchars, line, col);
    int64_t from = nyrt_str_offset(s, a), to = nyrt_str_offset(s, b);
    return nyrt_str_from(s->data + from, to - from);
}
// Byte position of `t` in `s` at or after byte `from`, or -1.
static int64_t nyrt_str_find(const nyrt_str *s, const nyrt_str *t, int64_t from) {
    NYRT_LIVE(s); NYRT_LIVE(t);
    if (t->len == 0) return from;
    for (int64_t i = from; i + t->len <= s->len; i++)
        if (memcmp(s->data + i, t->data, (size_t)t->len) == 0) return i;
    return -1;
}
static bool nyrt_str_contains(const nyrt_str *s, const nyrt_str *t) { return nyrt_str_find(s, t, 0) >= 0; }
static bool nyrt_str_starts_with(const nyrt_str *s, const nyrt_str *t) {
    return t->len <= s->len && memcmp(s->data, t->data, (size_t)t->len) == 0;
}
static bool nyrt_str_ends_with(const nyrt_str *s, const nyrt_str *t) {
    return t->len <= s->len && memcmp(s->data + s->len - t->len, t->data, (size_t)t->len) == 0;
}
static int64_t nyrt_str_index_of(const nyrt_str *s, const nyrt_str *t) {
    int64_t b = nyrt_str_find(s, t, 0);
    return b < 0 ? -1 : nyrt_utf8_count(s->data, b);
}
static nyrt_str *nyrt_str_replace(const nyrt_str *s, const nyrt_str *old, const nyrt_str *new_, int line, int col) {
    if (old->len == 0) nyrt_panic("E0243", "replace() needs a non-empty pattern", "the text to replace can't be \"\"", line, col);
    nyrt_buf b = nyrt_buf_new();
    int64_t i = 0, j;
    while ((j = nyrt_str_find(s, old, i)) >= 0) {
        nyrt_buf_add(&b, s->data + i, j - i);
        nyrt_buf_str(&b, new_);
        i = j + old->len;
    }
    nyrt_buf_add(&b, s->data + i, s->len - i);
    return nyrt_buf_done(&b);
}
static bool nyrt_is_space(nyrt_char c) { return c == ' ' || c == '\t' || c == '\n' || c == '\r'; }
static nyrt_str *nyrt_str_trim(const nyrt_str *s) {
    int64_t a = 0, b = s->len;
    while (a < b && nyrt_is_space((unsigned char)s->data[a])) a++;
    while (b > a && nyrt_is_space((unsigned char)s->data[b - 1])) b--;
    return nyrt_str_from(s->data + a, b - a);
}
// char tests: ASCII only, like upper() and lower()
static bool nyrt_char_is_digit(nyrt_char c) { return c >= '0' && c <= '9'; }
static bool nyrt_char_is_upper(nyrt_char c) { return c >= 'A' && c <= 'Z'; }
static bool nyrt_char_is_lower(nyrt_char c) { return c >= 'a' && c <= 'z'; }
static bool nyrt_char_is_letter(nyrt_char c) { return nyrt_char_is_upper(c) || nyrt_char_is_lower(c); }
static nyrt_char nyrt_char_upper(nyrt_char c) { return c >= 'a' && c <= 'z' ? c - 32 : c; }
static nyrt_char nyrt_char_lower(nyrt_char c) { return c >= 'A' && c <= 'Z' ? c + 32 : c; }
static nyrt_str *nyrt_str_upper(const nyrt_str *s) {
    nyrt_str *r = nyrt_str_from(s->data, s->len);
    for (int64_t i = 0; i < r->len; i++) r->data[i] = (char)nyrt_char_upper((unsigned char)r->data[i]);
    return r;
}
static nyrt_str *nyrt_str_lower(const nyrt_str *s) {
    nyrt_str *r = nyrt_str_from(s->data, s->len);
    for (int64_t i = 0; i < r->len; i++) r->data[i] = (char)nyrt_char_lower((unsigned char)r->data[i]);
    return r;
}
static nyrt_str *nyrt_str_repeat(const nyrt_str *s, int64_t n, int line, int col) {
    if (n < 0) {
        char msg[64];
        snprintf(msg, sizeof msg, "repeat count must be >= 0, got %lld", (long long)n);
        nyrt_panic("E0243", msg, "repeat(n) needs n >= 0", line, col);
    }
    NYRT_LIVE(s);
    // the whole size up front: a huge count fails at once instead of filling the memory first
    if (s->len > 0 && n > NYRT_MAX_STR / s->len) nyrt_oom(line, col);
    int64_t total = s->len * n;
    nyrt_str *r = nyrt_str_alloc_at(total, line, col);
    for (int64_t i = 0; i < n; i++) memcpy(r->data + i * s->len, s->data, (size_t)s->len);
    r->len = total;
    r->data[total] = '\0';
    r->nchars = s->nchars * n;
    return r;
}
// str(c): never allocates for ASCII (immortal one-character strings).
static nyrt_str *nyrt_char_str(nyrt_char c) {
    static nyrt_str ascii[128];
    static char bytes[128][2];
    if (c < 128) {
        nyrt_str *s = &ascii[c];
        if (!s->data) { bytes[c][0] = (char)c; s->data = bytes[c]; s->len = s->cap = s->nchars = 1; }
        return s;
    }
    char t[4];
    return nyrt_str_from(t, nyrt_utf8_encode(c, t));
}
static nyrt_char nyrt_char_from(int64_t n, int line, int col) {
    if (n < 0 || n > 1114111 || (n >= 55296 && n <= 57343)) {
        char msg[80];
        snprintf(msg, sizeof msg, "char(%lld): not a valid character code", (long long)n);
        nyrt_panic("E0246", msg, "character codes go from 0 to 1114111, except 55296 to 57343", line, col);
    }
    return (nyrt_char)n;
}
// The text of a string in an error message: control characters as escapes (`\n`, `\u0000`).
static void nyrt_buf_shown(nyrt_buf *b, const nyrt_str *s) {
    for (int64_t i = 0; i < s->len; i++) {
        unsigned char c = (unsigned char)s->data[i];
        char t[8];
        if (c == '\n') nyrt_buf_add(b, "\\n", 2);
        else if (c == '\t') nyrt_buf_add(b, "\\t", 2);
        else if (c == '\r') nyrt_buf_add(b, "\\r", 2);
        else if (c < 0x20) nyrt_buf_add(b, t, snprintf(t, sizeof t, "\\u%04x", c));
        else nyrt_buf_add(b, (const char *)&s->data[i], 1);
    }
}
// int(s): -?[0-9]+ that fits in an int, nothing else.
static int64_t nyrt_str_to_int(const nyrt_str *s, int line, int col) {
    int64_t i = 0, n = s->len;
    bool neg = n > 0 && s->data[0] == '-';
    if (neg) i = 1;
    uint64_t v = 0, limit = neg ? (uint64_t)INT64_MAX + 1 : (uint64_t)INT64_MAX;
    bool ok = i < n;
    for (; ok && i < n; i++) {
        char c = s->data[i];
        if (c < '0' || c > '9' || v > (limit - (uint64_t)(c - '0')) / 10) ok = false;
        else v = v * 10 + (uint64_t)(c - '0');
    }
    if (!ok) {
        nyrt_buf b = nyrt_buf_new();
        nyrt_buf_cstr(&b, "cannot parse \"");
        nyrt_buf_shown(&b, s);
        nyrt_buf_cstr(&b, "\" as int");
        nyrt_panic("E0244", nyrt_buf_done(&b)->data, "int(s) accepts only digits with an optional `-`, e.g. \"-42\"", line, col);
    }
    return neg ? (int64_t)(0 - v) : (int64_t)v;
}
// float(s): -?[0-9]+(.[0-9]+)?([eE][+-]?[0-9]+)?
static double nyrt_str_to_float(const nyrt_str *s, int line, int col) {
    const char *p = s->data, *e = s->data + s->len;
    bool ok = true;
    if (p < e && *p == '-') p++;
    const char *d = p;
    while (p < e && *p >= '0' && *p <= '9') p++;
    if (p == d) ok = false;
    if (ok && p < e && *p == '.') {
        d = ++p;
        while (p < e && *p >= '0' && *p <= '9') p++;
        if (p == d) ok = false;
    }
    if (ok && p < e && (*p == 'e' || *p == 'E')) {
        p++;
        if (p < e && (*p == '+' || *p == '-')) p++;
        d = p;
        while (p < e && *p >= '0' && *p <= '9') p++;
        if (p == d) ok = false;
    }
    if (!ok || p != e) {
        nyrt_buf b = nyrt_buf_new();
        nyrt_buf_cstr(&b, "cannot parse \"");
        nyrt_buf_shown(&b, s);
        nyrt_buf_cstr(&b, "\" as float");
        nyrt_panic("E0244", nyrt_buf_done(&b)->data, "float(s) accepts digits with an optional `-`, `.` part and exponent, e.g. \"-1.5e3\"", line, col);
    }
    return strtod(s->data, NULL);
}
