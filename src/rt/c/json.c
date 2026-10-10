// ---- json: `json.str(v)` and `json.parse(text)` by the value's type ------------------------
// Each type has an encoder `void (nyrt_buf *, const void *value)` and a decoder
// `void (nyrt_jp *, void *out)`; the compiler writes the ones for arrays and structs.
typedef struct nyrt_jseg { const char *key; int64_t index; } nyrt_jseg;   // key NULL: an index
typedef struct nyrt_jp {
    const char *s;
    int64_t n, i;
    int depth, plen, line, col;
    nyrt_jseg path[512];
} nyrt_jp;

static void nyrt_jenc_text(nyrt_buf *b, const char *s, int64_t n) {
    nyrt_buf_add(b, "\"", 1);
    for (int64_t k = 0; k < n; k++) {
        unsigned char c = (unsigned char)s[k];
        char t[8];
        if (c == '"') nyrt_buf_add(b, "\\\"", 2);
        else if (c == '\\') nyrt_buf_add(b, "\\\\", 2);
        else if (c == '\b') nyrt_buf_add(b, "\\b", 2);
        else if (c == '\f') nyrt_buf_add(b, "\\f", 2);
        else if (c == '\n') nyrt_buf_add(b, "\\n", 2);
        else if (c == '\r') nyrt_buf_add(b, "\\r", 2);
        else if (c == '\t') nyrt_buf_add(b, "\\t", 2);
        else if (c < 0x20) nyrt_buf_add(b, t, snprintf(t, sizeof t, "\\u%04x", c));
        else nyrt_buf_add(b, (const char *)&s[k], 1);
    }
    nyrt_buf_add(b, "\"", 1);
}
static void nyrt_jenc_int(nyrt_buf *b, const void *v) { nyrt_buf_int(b, *(const int64_t *)v); }
static void nyrt_jenc_float(nyrt_buf *b, const void *v) {
    double x = *(const double *)v;
    if (x != x || x > DBL_MAX || x < -DBL_MAX) nyrt_buf_cstr(b, "null");
    else nyrt_buf_float(b, x);
}
static void nyrt_jenc_bool(nyrt_buf *b, const void *v) { nyrt_buf_bool(b, *(const bool *)v); }
static void nyrt_jenc_char(nyrt_buf *b, const void *v) {
    char t[4];
    nyrt_jenc_text(b, t, nyrt_utf8_encode(*(const nyrt_char *)v, t));
}
static void nyrt_jenc_str(nyrt_buf *b, const void *v) {
    const nyrt_str *s = *(nyrt_str *const *)v;
    nyrt_jenc_text(b, s->data, s->len);
}
static void nyrt_jenc_arr(nyrt_buf *b, const void *v, void (*enc)(nyrt_buf *, const void *)) {
    const nyrt_arr *a = *(nyrt_arr *const *)v;
    nyrt_buf_add(b, "[", 1);
    for (int64_t k = 0; k < a->len; k++) {
        if (k) nyrt_buf_add(b, ",", 1);
        enc(b, a->data + k * a->ty->size);
    }
    nyrt_buf_add(b, "]", 1);
}

// ---- the parser -------------------------------------------------------------------------------
static void nyrt_jsyntax(nyrt_jp *p, const char *what) {
    int64_t end = p->i < p->n ? p->i : p->n;
    int line = 1;
    for (int64_t k = 0; k < end; k++) if (p->s[k] == '\n') line++;
    char msg[160];
    snprintf(msg, sizeof msg, "json.parse: invalid JSON at line %d: %s", line, what);
    nyrt_panic("E0345", msg, "check the JSON text: it must be one value, with keys and strings in double quotes", p->line, p->col);
}
static void nyrt_jpath(nyrt_jp *p, nyrt_buf *b) {
    nyrt_buf_add(b, "$", 1);
    for (int k = 0; k < p->plen; k++) {
        if (p->path[k].key) { nyrt_buf_add(b, ".", 1); nyrt_buf_cstr(b, p->path[k].key); }
        else { nyrt_buf_add(b, "[", 1); nyrt_buf_int(b, p->path[k].index); nyrt_buf_add(b, "]", 1); }
    }
}
static void nyrt_jtype(nyrt_jp *p, const char *what) {
    nyrt_buf b = nyrt_buf_new();
    nyrt_buf_cstr(&b, "json.parse: expected ");
    nyrt_buf_cstr(&b, what);
    nyrt_buf_cstr(&b, " at ");
    nyrt_jpath(p, &b);
    nyrt_panic("E0345", nyrt_buf_done(&b)->data, "the JSON text must have the shape of the type it is read into", p->line, p->col);
}
static void nyrt_jmissing(nyrt_jp *p, const char *field) {
    nyrt_buf b = nyrt_buf_new();
    nyrt_buf_cstr(&b, "json.parse: missing field \"");
    nyrt_buf_cstr(&b, field);
    nyrt_buf_cstr(&b, "\" at ");
    nyrt_jpath(p, &b);
    nyrt_panic("E0345", nyrt_buf_done(&b)->data, "the JSON object must have every field of the struct", p->line, p->col);
}
static void nyrt_jpush_key(nyrt_jp *p, const char *key) { p->path[p->plen].key = key; p->path[p->plen++].index = 0; }
static void nyrt_jpush_index(nyrt_jp *p, int64_t i) { p->path[p->plen].key = NULL; p->path[p->plen++].index = i; }
static void nyrt_jpop(nyrt_jp *p) { p->plen--; }
static int nyrt_jpeek(nyrt_jp *p) { return p->i < p->n ? (unsigned char)p->s[p->i] : -1; }
static void nyrt_jws(nyrt_jp *p) {
    for (int c = nyrt_jpeek(p); c == ' ' || c == '\t' || c == '\n' || c == '\r'; c = nyrt_jpeek(p)) p->i++;
}
// Skips white space to the start of a value: a syntax error unless one can start here.
static int nyrt_jstart(nyrt_jp *p) {
    nyrt_jws(p);
    int c = nyrt_jpeek(p);
    if (c < 0) nyrt_jsyntax(p, "unexpected end of the text");
    if (!c || !strchr("{[\"tfn-0123456789", c)) nyrt_jsyntax(p, "expected a value");
    return c;
}
// `[` or `{`: true if a first element follows, false for an empty one (already closed).
static bool nyrt_jopen(nyrt_jp *p, char open, const char *what) {
    if (nyrt_jstart(p) != open) nyrt_jtype(p, what);
    if (++p->depth > 500) nyrt_jsyntax(p, "nested too deeply");
    p->i++;
    nyrt_jws(p);
    if (nyrt_jpeek(p) == (open == '[' ? ']' : '}')) {
        p->i++;
        p->depth--;
        return false;
    }
    return true;
}
// After an element: true at `,` (another one follows), false at the closing bracket.
static bool nyrt_jnext(nyrt_jp *p, char close) {
    nyrt_jws(p);
    int c = nyrt_jpeek(p);
    if (c == ',') { p->i++; return true; }
    if (c == close) { p->i++; p->depth--; return false; }
    if (c < 0) nyrt_jsyntax(p, "unexpected end of the text");
    nyrt_jsyntax(p, close == ']' ? "expected `,` or `]`" : "expected `,` or `}`");
    return false;
}
static int nyrt_jhex(nyrt_jp *p, int64_t at) {
    if (at + 4 > p->n) return -1;
    int v = 0;
    for (int k = 0; k < 4; k++) {
        char c = p->s[at + k];
        int d = c >= '0' && c <= '9' ? c - '0' : c >= 'a' && c <= 'f' ? c - 'a' + 10 : c >= 'A' && c <= 'F' ? c - 'A' + 10 : -1;
        if (d < 0) return -1;
        v = v * 16 + d;
    }
    return v;
}
// A string; the position is at its `"`.
static nyrt_str *nyrt_jstring(nyrt_jp *p) {
    nyrt_buf b = nyrt_buf_new();
    p->i++;
    for (;;) {
        int c = nyrt_jpeek(p);
        if (c < 0) nyrt_jsyntax(p, "unterminated string");
        if (c == '"') { p->i++; return nyrt_buf_done(&b); }
        if (c < 0x20) nyrt_jsyntax(p, "control character in a string");
        if (c != '\\') {
            int64_t start = p->i;
            while (p->i < p->n && p->s[p->i] != '"' && p->s[p->i] != '\\' && (unsigned char)p->s[p->i] >= 0x20) p->i++;
            nyrt_buf_add(&b, p->s + start, p->i - start);
            continue;
        }
        p->i++;
        int e = nyrt_jpeek(p);
        if (e < 0) nyrt_jsyntax(p, "unterminated string");
        const char *from = "\"\\/bfnrt", *to = "\"\\/\b\f\n\r\t";
        const char *at = e ? strchr(from, e) : NULL;
        if (at) {
            nyrt_buf_add(&b, to + (at - from), 1);
            p->i++;
            continue;
        }
        if (e != 'u') nyrt_jsyntax(p, "invalid escape");
        int cp = nyrt_jhex(p, p->i + 1);
        if (cp < 0 || (cp >= 0xDC00 && cp <= 0xDFFF)) nyrt_jsyntax(p, "invalid escape");
        p->i += 5;
        if (cp >= 0xD800 && cp <= 0xDBFF) {
            int lo = p->i + 1 < p->n && p->s[p->i] == '\\' && p->s[p->i + 1] == 'u' ? nyrt_jhex(p, p->i + 2) : -1;
            if (lo < 0xDC00 || lo > 0xDFFF) nyrt_jsyntax(p, "invalid escape");
            cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
            p->i += 6;
        }
        nyrt_buf_char(&b, (nyrt_char)cp);
    }
}
// A number: where its text starts, and whether it has no fraction and no exponent.
static bool nyrt_jnumber(nyrt_jp *p, int64_t *start) {
    *start = p->i;
    bool whole = true;
#define NYRT_JDIGIT() (nyrt_jpeek(p) >= '0' && nyrt_jpeek(p) <= '9')
    if (nyrt_jpeek(p) == '-') p->i++;
    if (nyrt_jpeek(p) == '0') p->i++;
    else if (NYRT_JDIGIT()) { while (NYRT_JDIGIT()) p->i++; }
    else nyrt_jsyntax(p, "invalid number");
    if (nyrt_jpeek(p) == '.') {
        p->i++;
        if (!NYRT_JDIGIT()) nyrt_jsyntax(p, "invalid number");
        while (NYRT_JDIGIT()) p->i++;
        whole = false;
    }
    if (nyrt_jpeek(p) == 'e' || nyrt_jpeek(p) == 'E') {
        p->i++;
        if (nyrt_jpeek(p) == '+' || nyrt_jpeek(p) == '-') p->i++;
        if (!NYRT_JDIGIT()) nyrt_jsyntax(p, "invalid number");
        while (NYRT_JDIGIT()) p->i++;
        whole = false;
    }
#undef NYRT_JDIGIT
    return whole;
}
// `true` (1), `false` (0) or `null` (2).
static int nyrt_jliteral(nyrt_jp *p) {
    static const char *const words[3] = { "false", "true", "null" };
    for (int k = 0; k < 3; k++) {
        int64_t len = (int64_t)strlen(words[k]);
        if (p->n - p->i >= len && memcmp(p->s + p->i, words[k], (size_t)len) == 0) { p->i += len; return k; }
    }
    nyrt_jsyntax(p, "expected a value");
    return 2;
}
// An object's key and the `:` after it.
static nyrt_str *nyrt_jkey(nyrt_jp *p) {
    nyrt_jws(p);
    int c = nyrt_jpeek(p);
    if (c < 0) nyrt_jsyntax(p, "unexpected end of the text");
    if (c != '"') nyrt_jsyntax(p, "expected a string key");
    nyrt_str *k = nyrt_jstring(p);
    nyrt_jws(p);
    c = nyrt_jpeek(p);
    if (c < 0) nyrt_jsyntax(p, "unexpected end of the text");
    if (c != ':') nyrt_jsyntax(p, "expected `:`");
    p->i++;
    return k;
}
// Any value, only checked (an object's fields that the type does not have).
static void nyrt_jskip(nyrt_jp *p) {
    int c = nyrt_jstart(p);
    if (c == '{' || c == '[') {
        if (nyrt_jopen(p, (char)c, "")) {
            do {
                if (c == '{') nyrt_str_release(nyrt_jkey(p));
                nyrt_jskip(p);
            } while (nyrt_jnext(p, c == '{' ? '}' : ']'));
        }
    } else if (c == '"') nyrt_str_release(nyrt_jstring(p));
    else if (c == '-' || (c >= '0' && c <= '9')) { int64_t s; nyrt_jnumber(p, &s); }
    else nyrt_jliteral(p);
}

static void nyrt_jdec_int(nyrt_jp *p, void *out) {
    int c = nyrt_jstart(p);
    if (c != '-' && (c < '0' || c > '9')) nyrt_jtype(p, "an int");
    int64_t start;
    bool whole = nyrt_jnumber(p, &start);
    bool neg = p->s[start] == '-';
    uint64_t v = 0, limit = neg ? (uint64_t)INT64_MAX + 1 : (uint64_t)INT64_MAX;
    for (int64_t k = start + neg; whole && k < p->i; k++) {
        uint64_t d = (uint64_t)(p->s[k] - '0');
        if (v > (limit - d) / 10) whole = false;
        else v = v * 10 + d;
    }
    if (!whole) nyrt_jtype(p, "an int");
    *(int64_t *)out = neg ? (int64_t)(0 - v) : (int64_t)v;
}
static void nyrt_jdec_float(nyrt_jp *p, void *out) {
    int c = nyrt_jstart(p);
    if (c != '-' && (c < '0' || c > '9')) nyrt_jtype(p, "a number");
    int64_t start;
    nyrt_jnumber(p, &start);
    nyrt_str *t = nyrt_str_from(p->s + start, p->i - start);
    *(double *)out = strtod(t->data, NULL);
    nyrt_str_release(t);
}
static void nyrt_jdec_bool(nyrt_jp *p, void *out) {
    int c = nyrt_jstart(p);
    if (c != 't' && c != 'f') nyrt_jtype(p, "true or false");
    *(bool *)out = nyrt_jliteral(p) == 1;
}
static void nyrt_jdec_char(nyrt_jp *p, void *out) {
    if (nyrt_jstart(p) != '"') nyrt_jtype(p, "a one-character string");
    nyrt_str *s = nyrt_jstring(p);
    if (s->nchars != 1) nyrt_jtype(p, "a one-character string");
    int64_t adv;
    *(nyrt_char *)out = nyrt_utf8_decode(s->data, &adv);
    nyrt_str_release(s);
}
static void nyrt_jdec_str(nyrt_jp *p, void *out) {
    if (nyrt_jstart(p) != '"') nyrt_jtype(p, "a string");
    *(nyrt_str **)out = nyrt_jstring(p);
}
static void nyrt_jdec_arr(nyrt_jp *p, void *out, const nyrt_type *ty, void (*dec)(nyrt_jp *, void *)) {
    nyrt_arr *a = nyrt_arr_new(ty, 4);
    if (nyrt_jopen(p, '[', "an array")) {
        do {
            nyrt_jpush_index(p, a->len);
            nyrt_arr_grow(&a, 1);
            dec(p, a->data + a->len * ty->size);   // decoded straight into its place: the array owns it
            a->len++;
            nyrt_jpop(p);
        } while (nyrt_jnext(p, ']'));
    }
    *(nyrt_arr **)out = a;
}
// ---- tuples, optionals, enums and maps ---------------------------------------------------------
// `[`: an array of exactly `n` (>= 1) elements follows; any other value, or an empty array, is an error.
static void nyrt_jcount(nyrt_jp *p, int n) {
    char w[64];
    snprintf(w, sizeof w, "an array of %d elements", n);
    nyrt_jtype(p, w);
}
static void nyrt_jfixed_open(nyrt_jp *p, int n) {
    if (nyrt_jstart(p) != '[') nyrt_jcount(p, n);
    if (!nyrt_jopen(p, '[', "")) nyrt_jcount(p, n);
}
// After an element of such an array: another one follows unless it was the `last`.
static void nyrt_jfixed_next(nyrt_jp *p, int n, bool last) {
    if (nyrt_jnext(p, ']') == last) nyrt_jcount(p, n);
}
// `null` (true; consumed), or any other value (false; not consumed).
static bool nyrt_jnull(nyrt_jp *p) {
    if (nyrt_jstart(p) != 'n') return false;
    nyrt_jliteral(p);
    return true;
}
static void nyrt_jvariant(nyrt_jp *p, const char *name) {
    nyrt_buf b = nyrt_buf_new();
    nyrt_buf_cstr(&b, "a variant of ");
    nyrt_buf_cstr(&b, name);
    nyrt_buf_cstr(&b, ": a name, or {\"Name\": [values]}");
    nyrt_jtype(p, nyrt_buf_done(&b)->data);
}
static void nyrt_jenc_map(nyrt_buf *b, const void *v, void (*enck)(nyrt_buf *, const void *), void (*encv)(nyrt_buf *, const void *)) {
    const nyrt_map *m = *(nyrt_map *const *)v;
    NYRT_LIVE(m);
    bool obj = m->kt == &nyrt_T_str, first = true;
    nyrt_buf_add(b, obj ? "{" : "[", 1);
    for (int64_t i = 0; i < m->n; i++) {
        if (!m->alive[i]) continue;
        if (!first) nyrt_buf_add(b, ",", 1);
        first = false;
        if (!obj) nyrt_buf_add(b, "[", 1);
        enck(b, nyrt_map_ent(m, i));
        nyrt_buf_add(b, obj ? ":" : ",", 1);
        encv(b, nyrt_map_ent(m, i) + m->koff);
        if (!obj) nyrt_buf_add(b, "]", 1);
    }
    nyrt_buf_add(b, obj ? "}" : "]", 1);
}
static void nyrt_jdec_map(nyrt_jp *p, void *out, const nyrt_type *kt, const nyrt_type *vt, void (*deck)(nyrt_jp *, void *), void (*decv)(nyrt_jp *, void *)) {
    nyrt_map *m = nyrt_map_new(kt, vt);
    char *kb = malloc((size_t)kt->size + 8), *vb = malloc((size_t)vt->size + 8);
    if (!kb || !vb) nyrt_oom(p->line, p->col);
    if (kt == &nyrt_T_str) {
        if (nyrt_jopen(p, '{', "an object")) {
            do {
                nyrt_str *k = nyrt_jkey(p);
                nyrt_jpush_key(p, k->data);
                decv(p, vb);
                nyrt_jpop(p);
                nyrt_map_set(&m, &k, vb);
                nyrt_str_release(k);
                if (vt->release) vt->release(vb);
            } while (nyrt_jnext(p, '}'));
        }
    } else if (nyrt_jopen(p, '[', "an array")) {
        int64_t i = 0;
        do {
            nyrt_jpush_index(p, i++);
            nyrt_jfixed_open(p, 2);
            nyrt_jpush_index(p, 0);
            deck(p, kb);
            nyrt_jpop(p);
            nyrt_jfixed_next(p, 2, false);
            nyrt_jpush_index(p, 1);
            decv(p, vb);
            nyrt_jpop(p);
            nyrt_jfixed_next(p, 2, true);
            nyrt_jpop(p);
            nyrt_map_set(&m, kb, vb);
            if (kt->release) kt->release(kb);
            if (vt->release) vt->release(vb);
        } while (nyrt_jnext(p, ']'));
    }
    free(kb);
    free(vb);
    *(nyrt_map **)out = m;
}
static void nyrt_jparse(const nyrt_str *text, void (*dec)(nyrt_jp *, void *), void *out, int line, int col) {
    nyrt_jp *p = malloc(sizeof(nyrt_jp));
    if (!p) nyrt_oom(line, col);
    p->s = text->data; p->n = text->len; p->i = 0; p->depth = 0; p->plen = 0; p->line = line; p->col = col;
    dec(p, out);
    nyrt_jws(p);
    if (p->i < p->n) nyrt_jsyntax(p, "text after the value");
    free(p);
}
