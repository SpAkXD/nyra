// ---- arrays: reference counted, copy on write ---------------------------------------------
// Elements are stored inline after the header. Each array knows its element type: its size
// and, for managed elements (strings, arrays), how to retain and release one. A write first
// makes the array unique (`nyrt_arr_unique`), so no two owners ever see each other's changes.
typedef struct nyrt_type {
    int64_t size;
    void (*retain)(void *elem);                     // NULL: plain data
    void (*release)(void *elem);
    bool (*eq)(const void *a, const void *b);
    void (*fmt)(nyrt_buf *b, const void *elem);     // as an element: strings and chars quoted
    bool (*lt)(const void *a, const void *b);       // NULL: not sortable
    void (*keep)(void *elem);                       // `keep`: NULL for plain data
    uint64_t (*hash)(const void *elem);             // a tuple used as a map key: NULL for the others
} nyrt_type;
typedef struct nyrt_arr { int64_t rc, len, cap; const nyrt_type *ty; char *data; } nyrt_arr;
// The elements of an array as a C array of `T`: they always follow the header (`data` points there).
#define NYRT_ELEMS(T, a) ((T *)((a) + 1))
// The longest array `repeat` makes; the JavaScript runtime stops at the same length.
#define NYRT_MAX_ARR 100000000

static nyrt_arr *nyrt_arr_new_at(const nyrt_type *ty, int64_t cap, int line, int col) {
    nyrt_arr *a = nyrt_alloc_at(sizeof(nyrt_arr) + (size_t)(cap * ty->size), line, col);
    a->rc = 1; a->len = 0; a->cap = cap; a->ty = ty;
    a->data = (char *)(a + 1);
    return a;
}
static nyrt_arr *nyrt_arr_new(const nyrt_type *ty, int64_t cap) { return nyrt_arr_new_at(ty, cap, 0, 0); }
static inline void nyrt_arr_retain(nyrt_arr *a) {
    if (!a || !a->rc) return;
    if (a->rc < 0) nyrt_bug("a freed array was used again");
    a->rc++;
}
static void nyrt_arr_release(nyrt_arr *a) {
    if (!a || !a->rc) return;
    if (a->rc < 0) nyrt_bug("an array was freed twice");
    if (--a->rc > 0) return;
    if (a->ty->release) for (int64_t i = 0; i < a->len; i++) a->ty->release(a->data + i * a->ty->size);
    // a tombstone has no elements, so every later index check takes the slow path that reports it
    if (nyrt_checking) { a->rc = -1; a->len = 0; nyrt_live--; }
    else nyrt_free(a);
}
// `keep(xs)`: the array and everything in it is never freed (reference count 0), not a leak.
static void nyrt_arr_keep(nyrt_arr *a) {
    if (!a || a->rc <= 0) return;
    a->rc = 0;
    nyrt_live--;
    if (a->ty->keep) for (int64_t i = 0; i < a->len; i++) a->ty->keep(a->data + i * a->ty->size);
}
// Gives every element of a copy one more owner.
static void nyrt_arr_retain_all(nyrt_arr *a) {
    if (a->ty->retain) for (int64_t i = 0; i < a->len; i++) a->ty->retain(a->data + i * a->ty->size);
}
// Makes `*p` the only owner of its array: a shared array is copied first.
static NYRT_COLD void nyrt_arr_unique(nyrt_arr **p) {
    nyrt_arr *a = *p;
    NYRT_LIVE(a);
    if (a->rc == 1) return;
    nyrt_arr *c = nyrt_arr_new(a->ty, a->len);
    memcpy(c->data, a->data, (size_t)(a->len * a->ty->size));
    c->len = a->len;
    nyrt_arr_retain_all(c);
    nyrt_arr_release(a);
    *p = c;
}
// Room for `need` more elements in a unique array.
static void nyrt_arr_grow(nyrt_arr **p, int64_t need) {
    nyrt_arr *a = *p;
    if (a->len + need <= a->cap) return;
    int64_t cap = a->cap * 2;
    if (cap < a->len + need) cap = a->len + need;
    if (cap < 4) cap = 4;
    a = nyrt_realloc(a, sizeof(nyrt_arr) + (size_t)(cap * a->ty->size));
    a->data = (char *)(a + 1);
    a->cap = cap;
    *p = a;
}
static NYRT_NORETURN void nyrt_oob(int64_t i, int64_t n, int line, int col) {
    char msg[96];
    snprintf(msg, sizeof msg, "index %lld is out of bounds for length %lld", (long long)i, (long long)n);
    nyrt_panic("E0240", msg, "valid indexes are 0 to len - 1; compare with `.len()` first", line, col);
}
// The address of element `i` (E0240 when out of bounds).
static void *nyrt_arr_at(const nyrt_arr *a, int64_t i, int line, int col) {
    NYRT_LIVE(a);
    if (i < 0 || i >= a->len) nyrt_oob(i, a->len, line, col);
    return a->data + i * a->ty->size;
}
// (the position is packed into one argument, see NYRT_AT: a call with fewer arguments is smaller
// at every index check, and the C compiler inlines and unrolls more around them)
static NYRT_NORETURN void nyrt_arr_bad_index(const nyrt_arr *a, int64_t i, int64_t at) {
    NYRT_LIVE(a);
    nyrt_oob(i, a->len, (int)(at >> 32), (int)((at >> 8) & 0xffffff));
}
// The generated code reads and writes elements as `NYRT_ELEMS(T, a)[nyrt_ix(a, i, line, col)]`:
// `i` checked against the length (E0240), with the error path out of line.
static inline int64_t nyrt_ix(const nyrt_arr *a, int64_t i, int line, int col) {
    if (NYRT_UNLIKELY((uint64_t)i >= (uint64_t)a->len)) nyrt_arr_bad_index(a, i, NYRT_AT(line, col, 0));
    return i;
}
// The same with the length read before a loop that cannot change it (`n`).
static inline int64_t nyrt_ixn(const nyrt_arr *a, int64_t i, int64_t n, int line, int col) {
    if (NYRT_UNLIKELY((uint64_t)i >= (uint64_t)n)) nyrt_arr_bad_index(a, i, NYRT_AT(line, col, 0));
    return i;
}
// Before a write: makes `*p` the only owner (the copy, like the memory check, is the slow path).
static inline void nyrt_arr_mut(nyrt_arr **p) {
    if (NYRT_UNLIKELY((*p)->rc != 1)) nyrt_arr_unique(p);
}
static NYRT_NORETURN void nyrt_pop_empty(int line, int col) {
    nyrt_panic("E0242", "pop() on an empty array", "check `xs.len() > 0` first", line, col);
}

// ---- changing an array (the caller made it unique first) ---------------------------------
static void nyrt_arr_push(nyrt_arr **p, const void *elem) {
    nyrt_arr_grow(p, 1);
    nyrt_arr *a = *p;
    char *slot = a->data + a->len * a->ty->size;
    memcpy(slot, elem, (size_t)a->ty->size);
    if (a->ty->retain) a->ty->retain(slot);
    a->len++;
}
// The last element moves out into `out` (it keeps its owner count).
static void nyrt_arr_pop(nyrt_arr **p, void *out, int line, int col) {
    nyrt_arr *a = *p;
    if (a->len == 0) nyrt_pop_empty(line, col);
    a->len--;
    memcpy(out, a->data + a->len * a->ty->size, (size_t)a->ty->size);
}
static void nyrt_arr_insert(nyrt_arr **p, int64_t i, const void *elem, int line, int col) {
    nyrt_arr *a = *p;
    if (i < 0 || i > a->len) {
        char msg[96];
        snprintf(msg, sizeof msg, "index %lld is out of bounds for length %lld", (long long)i, (long long)a->len);
        nyrt_panic("E0240", msg, "insert(i, x) needs 0 <= i <= len", line, col);
    }
    nyrt_arr_grow(p, 1);
    a = *p;
    int64_t sz = a->ty->size;
    memmove(a->data + (i + 1) * sz, a->data + i * sz, (size_t)((a->len - i) * sz));
    memcpy(a->data + i * sz, elem, (size_t)sz);
    if (a->ty->retain) a->ty->retain(a->data + i * sz);
    a->len++;
}
// Element `i` moves out into `out`.
static void nyrt_arr_remove(nyrt_arr **p, int64_t i, void *out, int line, int col) {
    nyrt_arr *a = *p;
    if (i < 0 || i >= a->len) nyrt_oob(i, a->len, line, col);
    int64_t sz = a->ty->size;
    memcpy(out, a->data + i * sz, (size_t)sz);
    memmove(a->data + i * sz, a->data + (i + 1) * sz, (size_t)((a->len - i - 1) * sz));
    a->len--;
}
static void nyrt_arr_swap(nyrt_arr **p, int64_t i, int64_t j, int line, int col) {
    nyrt_arr *a = *p;
    if (i < 0 || i >= a->len) nyrt_oob(i, a->len, line, col);
    if (j < 0 || j >= a->len) nyrt_oob(j, a->len, line, col);
    int64_t sz = a->ty->size;
    char *x = a->data + i * sz, *y = a->data + j * sz;
    for (int64_t k = 0; k < sz; k++) { char c = x[k]; x[k] = y[k]; y[k] = c; }
}
static void nyrt_arr_reverse(nyrt_arr **p) {
    nyrt_arr *a = *p;
    int64_t sz = a->ty->size;
    for (int64_t i = 0, j = a->len - 1; i < j; i++, j--) {
        char *x = a->data + i * sz, *y = a->data + j * sz;
        for (int64_t k = 0; k < sz; k++) { char c = x[k]; x[k] = y[k]; y[k] = c; }
    }
}
// Top-down merge sort: stable, and it takes from the left unless the right element is smaller,
// so even NaN ends up in the same place as in the JavaScript runtime (same algorithm there).
static void nyrt_merge_sort(char *a, char *tmp, int64_t lo, int64_t hi, int64_t sz, bool (*lt)(const void *, const void *)) {
    if (hi - lo < 2) return;
    int64_t mid = lo + (hi - lo) / 2;
    nyrt_merge_sort(a, tmp, lo, mid, sz, lt);
    nyrt_merge_sort(a, tmp, mid, hi, sz, lt);
    int64_t i = lo, j = mid;
    for (int64_t k = lo; k < hi; k++) {
        if (j < hi && (i >= mid || lt(a + j * sz, a + i * sz))) { memcpy(tmp + k * sz, a + j * sz, (size_t)sz); j++; }
        else { memcpy(tmp + k * sz, a + i * sz, (size_t)sz); i++; }
    }
    memcpy(a + lo * sz, tmp + lo * sz, (size_t)((hi - lo) * sz));
}
// `xs += ys`: appends in place when `*p` is the only owner; `xs += xs` doubles it.
static void nyrt_arr_append(nyrt_arr **p, const nyrt_arr *b) {
    nyrt_arr *keep = (nyrt_arr *)b;
    nyrt_arr_retain(keep);                 // `b` stays as it is, even when it is `*p`
    nyrt_arr_unique(p);
    nyrt_arr_grow(p, b->len);
    nyrt_arr *a = *p;
    int64_t sz = a->ty->size;
    memcpy(a->data + a->len * sz, b->data, (size_t)(b->len * sz));
    if (a->ty->retain) for (int64_t i = 0; i < b->len; i++) a->ty->retain(a->data + (a->len + i) * sz);
    a->len += b->len;
    nyrt_arr_release(keep);
}

// ---- new arrays from arrays ----------------------------------------------------------------
static nyrt_arr *nyrt_arr_slice(const nyrt_arr *a, int64_t from, int64_t to, int line, int col) {
    NYRT_LIVE(a);
    nyrt_range_check(from, to, a->len, line, col);
    int64_t sz = a->ty->size;
    nyrt_arr *r = nyrt_arr_new(a->ty, to - from);
    memcpy(r->data, a->data + from * sz, (size_t)((to - from) * sz));
    r->len = to - from;
    nyrt_arr_retain_all(r);
    return r;
}
static nyrt_arr *nyrt_arr_concat(const nyrt_arr *a, const nyrt_arr *b) {
    NYRT_LIVE(a); NYRT_LIVE(b);
    int64_t sz = a->ty->size;
    nyrt_arr *r = nyrt_arr_new(a->ty, a->len + b->len);
    memcpy(r->data, a->data, (size_t)(a->len * sz));
    memcpy(r->data + a->len * sz, b->data, (size_t)(b->len * sz));
    r->len = a->len + b->len;
    nyrt_arr_retain_all(r);
    return r;
}
static nyrt_arr *nyrt_arr_repeat(const nyrt_arr *a, int64_t n, int line, int col) {
    NYRT_LIVE(a);
    if (n < 0) {
        char msg[64];
        snprintf(msg, sizeof msg, "repeat count must be >= 0, got %lld", (long long)n);
        nyrt_panic("E0243", msg, "repeat(n) needs n >= 0", line, col);
    }
    if (a->len > 0 && n > NYRT_MAX_ARR / a->len) nyrt_oom(line, col);
    int64_t sz = a->ty->size, block = a->len * sz, total = block * n;
    nyrt_arr *r = nyrt_arr_new_at(a->ty, a->len * n, line, col);
    // one copy, then the copied part doubles: a few big copies instead of n small ones
    if (total > 0) {
        memcpy(r->data, a->data, (size_t)block);
        for (int64_t done = block; done < total; done *= 2)
            memcpy(r->data + done, r->data, (size_t)(done < total - done ? done : total - done));
    }
    r->len = a->len * n;
    nyrt_arr_retain_all(r);
    return r;
}

// ---- reading -------------------------------------------------------------------------------
// Deep equality, element by element (no shortcut for the same array: NaN never equals itself).
static bool nyrt_arr_eq(const nyrt_arr *a, const nyrt_arr *b) {
    NYRT_LIVE(a); NYRT_LIVE(b);
    if (a->len != b->len) return false;
    int64_t sz = a->ty->size;
    for (int64_t i = 0; i < a->len; i++)
        if (!a->ty->eq(a->data + i * sz, b->data + i * sz)) return false;
    return true;
}
static int64_t nyrt_arr_index_of(const nyrt_arr *a, const void *elem) {
    NYRT_LIVE(a);
    for (int64_t i = 0; i < a->len; i++)
        if (a->ty->eq(a->data + i * a->ty->size, elem)) return i;
    return -1;
}
static bool nyrt_arr_contains(const nyrt_arr *a, const void *elem) { return nyrt_arr_index_of(a, elem) >= 0; }
static void nyrt_buf_arr(nyrt_buf *b, const nyrt_arr *a) {
    NYRT_LIVE(a);
    nyrt_buf_add(b, "[", 1);
    for (int64_t i = 0; i < a->len; i++) {
        if (i) nyrt_buf_add(b, ", ", 2);
        a->ty->fmt(b, a->data + i * a->ty->size);
    }
    nyrt_buf_add(b, "]", 1);
}
static void nyrt_put_arr(const nyrt_arr *a) {
    nyrt_buf b = nyrt_buf_new();
    nyrt_buf_arr(&b, a);
    fwrite(b.s->data, 1, (size_t)b.s->len, stdout);
    nyrt_str_release(b.s);
}
// Prints a value with its format function (a struct).
static void nyrt_put_fmt(void (*fmt)(nyrt_buf *, const void *), const void *v) {
    nyrt_buf b = nyrt_buf_new();
    fmt(&b, v);
    fwrite(b.s->data, 1, (size_t)b.s->len, stdout);
    nyrt_str_release(b.s);
}
// A string or a char as it is written in Nyra code: quoted, with escapes.
static void nyrt_buf_escaped(nyrt_buf *b, nyrt_char c, char quote) {
    switch (c) {
        case '\\': nyrt_buf_add(b, "\\\\", 2); break;
        case '\n': nyrt_buf_add(b, "\\n", 2); break;
        case '\t': nyrt_buf_add(b, "\\t", 2); break;
        case '\r': nyrt_buf_add(b, "\\r", 2); break;
        default:
            if (c == (nyrt_char)quote) { char e[2] = { '\\', quote }; nyrt_buf_add(b, e, 2); }
            else nyrt_buf_char(b, c);
    }
}
static void nyrt_buf_repr_str(nyrt_buf *b, const nyrt_str *s) {
    NYRT_LIVE(s);
    nyrt_buf_add(b, "\"", 1);
    for (int64_t k = 0; k < s->len; ) {
        int64_t adv;
        nyrt_buf_escaped(b, nyrt_utf8_decode(s->data + k, &adv), '"');
        k += adv;
    }
    nyrt_buf_add(b, "\"", 1);
}
static void nyrt_buf_repr_char(nyrt_buf *b, nyrt_char c) {
    nyrt_buf_add(b, "'", 1);
    nyrt_buf_escaped(b, c, '\'');
    nyrt_buf_add(b, "'", 1);
}

// ---- element types ---------------------------------------------------------------------------
static void nyrt_retain_str(void *e) { nyrt_str_retain(*(nyrt_str **)e); }
static void nyrt_release_str(void *e) { nyrt_str_release(*(nyrt_str **)e); }
static void nyrt_retain_arr(void *e) { nyrt_arr_retain(*(nyrt_arr **)e); }
static void nyrt_release_arr(void *e) { nyrt_arr_release(*(nyrt_arr **)e); }
static void nyrt_keep_str(void *e) { nyrt_str_keep(*(nyrt_str **)e); }
static void nyrt_keep_arr(void *e) { nyrt_arr_keep(*(nyrt_arr **)e); }
static bool nyrt_eq_int(const void *a, const void *b) { return *(const int64_t *)a == *(const int64_t *)b; }
static bool nyrt_eq_float(const void *a, const void *b) { return *(const double *)a == *(const double *)b; }
static bool nyrt_eq_bool(const void *a, const void *b) { return *(const bool *)a == *(const bool *)b; }
static bool nyrt_eq_char(const void *a, const void *b) { return *(const nyrt_char *)a == *(const nyrt_char *)b; }
static bool nyrt_eq_str(const void *a, const void *b) { return nyrt_str_eq(*(nyrt_str *const *)a, *(nyrt_str *const *)b); }
static bool nyrt_eq_arr(const void *a, const void *b) { return nyrt_arr_eq(*(nyrt_arr *const *)a, *(nyrt_arr *const *)b); }
static void nyrt_fmt_int(nyrt_buf *b, const void *e) { nyrt_buf_int(b, *(const int64_t *)e); }
static void nyrt_fmt_float(nyrt_buf *b, const void *e) { nyrt_buf_float(b, *(const double *)e); }
static void nyrt_fmt_bool(nyrt_buf *b, const void *e) { nyrt_buf_bool(b, *(const bool *)e); }
static void nyrt_fmt_char(nyrt_buf *b, const void *e) { nyrt_buf_repr_char(b, *(const nyrt_char *)e); }
static void nyrt_fmt_str(nyrt_buf *b, const void *e) { nyrt_buf_repr_str(b, *(nyrt_str *const *)e); }
static void nyrt_fmt_arr(nyrt_buf *b, const void *e) { nyrt_buf_arr(b, *(nyrt_arr *const *)e); }
static bool nyrt_lt_int(const void *a, const void *b) { return *(const int64_t *)a < *(const int64_t *)b; }
// NaN sorts after every number (and NaNs keep their order), on every backend
#define NYRT_LT_FLOAT(x, y) ((x) < (y) || ((y) != (y) && (x) == (x)))
static bool nyrt_lt_float(const void *a, const void *b) { return NYRT_LT_FLOAT(*(const double *)a, *(const double *)b); }
static bool nyrt_lt_char(const void *a, const void *b) { return *(const nyrt_char *)a < *(const nyrt_char *)b; }
static bool nyrt_lt_str(const void *a, const void *b) { return nyrt_str_cmp(*(nyrt_str *const *)a, *(nyrt_str *const *)b) < 0; }
static const nyrt_type nyrt_T_int = { sizeof(int64_t), NULL, NULL, nyrt_eq_int, nyrt_fmt_int, nyrt_lt_int, NULL };
static const nyrt_type nyrt_T_float = { sizeof(double), NULL, NULL, nyrt_eq_float, nyrt_fmt_float, nyrt_lt_float, NULL };
static const nyrt_type nyrt_T_bool = { sizeof(bool), NULL, NULL, nyrt_eq_bool, nyrt_fmt_bool, NULL, NULL };
static const nyrt_type nyrt_T_char = { sizeof(nyrt_char), NULL, NULL, nyrt_eq_char, nyrt_fmt_char, nyrt_lt_char, NULL };
static const nyrt_type nyrt_T_str = {
    sizeof(nyrt_str *), nyrt_retain_str, nyrt_release_str, nyrt_eq_str, nyrt_fmt_str, nyrt_lt_str, nyrt_keep_str
};
static const nyrt_type nyrt_T_arr = {
    sizeof(nyrt_arr *), nyrt_retain_arr, nyrt_release_arr, nyrt_eq_arr, nyrt_fmt_arr, NULL, nyrt_keep_arr
};

// ---- strings to arrays and back ----------------------------------------------------------------
static nyrt_arr *nyrt_str_chars(const nyrt_str *s) {
    NYRT_LIVE(s);
    nyrt_arr *r = nyrt_arr_new(&nyrt_T_char, s->nchars);
    for (int64_t k = 0; k < s->len; ) {
        int64_t adv;
        ((nyrt_char *)r->data)[r->len++] = nyrt_utf8_decode(s->data + k, &adv);
        k += adv;
    }
    return r;
}
static nyrt_arr *nyrt_str_codes(const nyrt_str *s) {
    NYRT_LIVE(s);
    nyrt_arr *r = nyrt_arr_new(&nyrt_T_int, s->nchars);
    for (int64_t k = 0; k < s->len; ) {
        int64_t adv;
        ((int64_t *)r->data)[r->len++] = (int64_t)nyrt_utf8_decode(s->data + k, &adv);
        k += adv;
    }
    return r;
}
static nyrt_arr *nyrt_str_split(const nyrt_str *s, const nyrt_str *sep, int line, int col) {
    if (sep->len == 0)
        nyrt_panic("E0243", "split() needs a non-empty separator", "for the characters of a string use `s.chars()`", line, col);
    nyrt_arr *r = nyrt_arr_new(&nyrt_T_str, 4);
    int64_t i = 0, j;
    while ((j = nyrt_str_find(s, sep, i)) >= 0) {
        nyrt_arr_grow(&r, 1);
        ((nyrt_str **)r->data)[r->len++] = nyrt_str_from(s->data + i, j - i);   // moves in
        i = j + sep->len;
    }
    nyrt_arr_grow(&r, 1);
    ((nyrt_str **)r->data)[r->len++] = nyrt_str_from(s->data + i, s->len - i);
    return r;
}
static nyrt_str *nyrt_arr_join(const nyrt_arr *a, const nyrt_str *sep) {
    NYRT_LIVE(a);
    if (a->ty == &nyrt_T_str) {
        // the size first, then one allocation
        nyrt_str **xs = NYRT_ELEMS(nyrt_str *, a);
        int64_t len = 0, nchars = 0;
        for (int64_t i = 0; i < a->len; i++) { NYRT_LIVE(xs[i]); len += xs[i]->len; nchars += xs[i]->nchars; }
        if (a->len > 1) { len += sep->len * (a->len - 1); nchars += sep->nchars * (a->len - 1); }
        nyrt_str *r = nyrt_str_alloc(len);
        char *o = r->data;
        for (int64_t i = 0; i < a->len; i++) {
            if (i) { memcpy(o, sep->data, (size_t)sep->len); o += sep->len; }
            memcpy(o, xs[i]->data, (size_t)xs[i]->len);
            o += xs[i]->len;
        }
        *o = '\0';
        r->len = len;
        r->nchars = nchars;
        return r;
    }
    nyrt_buf b = nyrt_buf_new();
    for (int64_t i = 0; i < a->len; i++) {
        if (i) nyrt_buf_str(&b, sep);
        if (a->ty == &nyrt_T_char) nyrt_buf_char(&b, ((nyrt_char *)a->data)[i]);
        else nyrt_buf_str(&b, ((nyrt_str **)a->data)[i]);
    }
    return nyrt_buf_done(&b);
}

// ---- sorting (after the element types it dispatches on) ----------------------------------------
// A stable merge sort for the sortable element types, without a call per comparison: insertion
// sort for short runs, no merge when the halves are already in order, and only the left half
// copied out. Every stable sort gives the same result (the orders are strict weak orders, NaN
// last), so the output matches the plain merge sort of the other backends.
#define NYRT_MSORT(name, T, LT) \
    static void name(T *a, T *tmp, int64_t lo, int64_t hi) { \
        if (hi - lo <= 16) { \
            for (int64_t i = lo + 1; i < hi; i++) { \
                T x = a[i]; \
                int64_t j = i; \
                while (j > lo && LT(x, a[j - 1])) { a[j] = a[j - 1]; j--; } \
                a[j] = x; \
            } \
            return; \
        } \
        int64_t mid = lo + (hi - lo) / 2; \
        name(a, tmp, lo, mid); \
        name(a, tmp, mid, hi); \
        if (!LT(a[mid], a[mid - 1])) return; \
        memcpy(tmp + lo, a + lo, (size_t)(mid - lo) * sizeof(T)); \
        int64_t i = lo, j = mid, k = lo; \
        while (i < mid && j < hi) { /* no branch: the comparison picks the side */ \
            T l = tmp[i]; \
            T r = a[j]; \
            int right = LT(r, l); \
            a[k++] = right ? r : l; \
            j += right; \
            i += !right; \
        } \
        while (i < mid) a[k++] = tmp[i++]; \
    }
#define NYRT_LT(x, y) ((x) < (y))
#define NYRT_LT_STR(x, y) (nyrt_str_cmp((x), (y)) < 0)
NYRT_MSORT(nyrt_msort_int, int64_t, NYRT_LT)
NYRT_MSORT(nyrt_msort_float, double, NYRT_LT_FLOAT)
NYRT_MSORT(nyrt_msort_char, nyrt_char, NYRT_LT)
NYRT_MSORT(nyrt_msort_str, nyrt_str *, NYRT_LT_STR)
static void nyrt_arr_sort(nyrt_arr **p) {
    nyrt_arr *a = *p;
    if (a->len < 2) return;
    char *tmp = malloc((size_t)(a->len * a->ty->size));
    if (!tmp) nyrt_oom(0, 0);
    if (a->ty == &nyrt_T_int) nyrt_msort_int((int64_t *)a->data, (int64_t *)tmp, 0, a->len);
    else if (a->ty == &nyrt_T_float) nyrt_msort_float((double *)a->data, (double *)tmp, 0, a->len);
    else if (a->ty == &nyrt_T_char) nyrt_msort_char((nyrt_char *)a->data, (nyrt_char *)tmp, 0, a->len);
    else if (a->ty == &nyrt_T_str) nyrt_msort_str((nyrt_str **)a->data, (nyrt_str **)tmp, 0, a->len);
    else nyrt_merge_sort(a->data, tmp, 0, a->len, a->ty->size, a->ty->lt);
    free(tmp);
}
// `xs.sort_by(x => key)`: the same merge sort on the positions 0..len, ordered by `keys` (one key
// per element), then the elements move to their new places (no reference count changes).
static void nyrt_msort_by(int64_t *a, int64_t *tmp, int64_t lo, int64_t hi, const char *k, int64_t ksz, bool (*lt)(const void *, const void *)) {
    if (hi - lo < 2) return;
    int64_t mid = lo + (hi - lo) / 2;
    nyrt_msort_by(a, tmp, lo, mid, k, ksz, lt);
    nyrt_msort_by(a, tmp, mid, hi, k, ksz, lt);
    int64_t i = lo, j = mid;
    for (int64_t n = lo; n < hi; n++) tmp[n] = (j < hi && (i >= mid || lt(k + a[j] * ksz, k + a[i] * ksz))) ? a[j++] : a[i++];
    memcpy(a + lo, tmp + lo, (size_t)(hi - lo) * sizeof(int64_t));
}
static void nyrt_arr_sort_by(nyrt_arr **p, const nyrt_arr *keys) {
    nyrt_arr *a = *p;
    int64_t n = a->len, sz = a->ty->size;
    if (n < 2) return;
    int64_t *idx = malloc((size_t)n * sizeof(int64_t) * 2);
    char *moved = malloc((size_t)(n * sz));
    if (!idx || !moved) nyrt_oom(0, 0);
    for (int64_t i = 0; i < n; i++) idx[i] = i;
    nyrt_msort_by(idx, idx + n, 0, n, keys->data, keys->ty->size, keys->ty->lt);
    for (int64_t i = 0; i < n; i++) memcpy(moved + i * sz, a->data + idx[i] * sz, (size_t)sz);
    memcpy(a->data, moved, (size_t)(n * sz));
    free(idx);
    free(moved);
}
