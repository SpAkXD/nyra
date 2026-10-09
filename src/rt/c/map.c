// ---- maps: reference counted, copy on write, entries in insertion order -------------------------
// Each entry is a key and a value stored inline, with an `alive` mark (a removed entry is a gap
// until the next compaction). An open-addressing table of entry numbers (+1; 0 is empty) finds a
// key. Keys are ints, strings, chars or bools; the key and value types are descriptors like an
// array's element type.
typedef struct nyrt_map {
    int64_t rc, len;            // live entries
    int64_t n, cap;             // entries used (gaps included), room for entries
    const nyrt_type *kt, *vt;
    int64_t koff, esize;        // the value's offset in an entry, the size of an entry
    char *ents;
    bool *alive;
    int64_t *slots, nslots;     // a power of two, at least twice n
} nyrt_map;

static int64_t nyrt_align8(int64_t n) { return (n + 7) & ~(int64_t)7; }
static uint64_t nyrt_key_hash(const nyrt_type *kt, const void *k) {
    uint64_t h;
    if (kt == &nyrt_T_str) {
        const nyrt_str *s = *(nyrt_str *const *)k;
        h = 1469598103934665603ull;
        for (int64_t i = 0; i < s->len; i++) { h ^= (unsigned char)s->data[i]; h *= 1099511628211ull; }
    } else if (kt == &nyrt_T_int) h = (uint64_t)*(const int64_t *)k;
    else if (kt == &nyrt_T_char) h = *(const nyrt_char *)k;
    else h = *(const bool *)k;
    h ^= h >> 33; h *= 0xff51afd7ed558ccdull; h ^= h >> 33;
    return h;
}
static nyrt_map *nyrt_map_new(const nyrt_type *kt, const nyrt_type *vt) {
    nyrt_map *m = nyrt_alloc(sizeof(nyrt_map));
    m->rc = 1; m->len = 0; m->n = 0; m->cap = 0; m->kt = kt; m->vt = vt;
    m->koff = nyrt_align8(kt->size);
    m->esize = m->koff + nyrt_align8(vt->size);
    m->ents = NULL; m->alive = NULL; m->slots = NULL; m->nslots = 0;
    return m;
}
static char *nyrt_map_ent(const nyrt_map *m, int64_t i) { return m->ents + i * m->esize; }
// The entry with key `k`, or -1.
static int64_t nyrt_map_find(const nyrt_map *m, const void *k) {
    if (!m->nslots) return -1;
    uint64_t mask = (uint64_t)m->nslots - 1, at = nyrt_key_hash(m->kt, k) & mask;
    for (;;) {
        int64_t e = m->slots[at];
        if (e == 0) return -1;
        if (m->alive[e - 1] && m->kt->eq(nyrt_map_ent(m, e - 1), k)) return e - 1;
        at = (at + 1) & mask;
    }
}
static void nyrt_map_reindex(nyrt_map *m, int64_t nslots) {
    free(m->slots);
    m->slots = calloc((size_t)nslots, sizeof(int64_t));
    if (!m->slots) nyrt_oom(0, 0);
    m->nslots = nslots;
    uint64_t mask = (uint64_t)nslots - 1;
    for (int64_t i = 0; i < m->n; i++) {
        if (!m->alive[i]) continue;
        uint64_t at = nyrt_key_hash(m->kt, nyrt_map_ent(m, i)) & mask;
        while (m->slots[at]) at = (at + 1) & mask;
        m->slots[at] = i + 1;
    }
}
static void nyrt_map_retain(nyrt_map *m) {
    if (!m || !m->rc) return;
    if (m->rc < 0) nyrt_bug("a freed map was used again");
    m->rc++;
}
static void nyrt_map_release(nyrt_map *m) {
    if (!m || !m->rc) return;
    if (m->rc < 0) nyrt_bug("a map was freed twice");
    if (--m->rc > 0) return;
    for (int64_t i = 0; i < m->n; i++) {
        if (!m->alive[i]) continue;
        if (m->kt->release) m->kt->release(nyrt_map_ent(m, i));
        if (m->vt->release) m->vt->release(nyrt_map_ent(m, i) + m->koff);
    }
    free(m->ents); free(m->alive); free(m->slots);
    m->ents = NULL; m->alive = NULL; m->slots = NULL;
    if (nyrt_checking) { m->rc = -1; nyrt_live--; }   // tombstone
    else nyrt_free(m);
}
// `keep(m)`: never freed, with everything in it.
static void nyrt_map_keep(nyrt_map *m) {
    if (!m || m->rc <= 0) return;
    m->rc = 0;
    nyrt_live--;
    for (int64_t i = 0; i < m->n; i++) {
        if (!m->alive[i]) continue;
        if (m->kt->keep) m->kt->keep(nyrt_map_ent(m, i));
        if (m->vt->keep) m->vt->keep(nyrt_map_ent(m, i) + m->koff);
    }
}
// Adds an entry at the end (the key is new); the map becomes one more owner of both.
static void nyrt_map_append(nyrt_map *m, const void *k, const void *v) {
    if (m->n == m->cap) {
        m->cap = m->cap ? m->cap * 2 : 4;
        m->ents = realloc(m->ents, (size_t)(m->cap * m->esize));
        m->alive = realloc(m->alive, (size_t)m->cap);
        if (!m->ents || !m->alive) nyrt_oom(0, 0);
    }
    char *e = nyrt_map_ent(m, m->n);
    memcpy(e, k, (size_t)m->kt->size);
    memcpy(e + m->koff, v, (size_t)m->vt->size);
    if (m->kt->retain) m->kt->retain(e);
    if (m->vt->retain) m->vt->retain(e + m->koff);
    m->alive[m->n] = true;
    m->n++;
    m->len++;
    if (2 * m->n > m->nslots) nyrt_map_reindex(m, m->nslots ? m->nslots * 2 : 8);
    else {
        uint64_t mask = (uint64_t)m->nslots - 1, at = nyrt_key_hash(m->kt, e) & mask;
        while (m->slots[at]) at = (at + 1) & mask;
        m->slots[at] = m->n;
    }
}
// A compact copy, each entry with one more owner.
static nyrt_map *nyrt_map_copy(const nyrt_map *m) {
    nyrt_map *c = nyrt_map_new(m->kt, m->vt);
    for (int64_t i = 0; i < m->n; i++)
        if (m->alive[i]) nyrt_map_append(c, nyrt_map_ent(m, i), nyrt_map_ent(m, i) + m->koff);
    return c;
}
// Makes `*p` the only owner of its map: a shared map is copied first.
static void nyrt_map_unique(nyrt_map **p) {
    nyrt_map *m = *p;
    NYRT_LIVE(m);
    if (m->rc == 1) return;
    *p = nyrt_map_copy(m);
    nyrt_map_release(m);
}
// `m[k] = v`: an existing key keeps its place; a new one goes last.
static void nyrt_map_set(nyrt_map **p, const void *k, const void *v) {
    nyrt_map_unique(p);
    nyrt_map *m = *p;
    int64_t i = nyrt_map_find(m, k);
    if (i < 0) { nyrt_map_append(m, k, v); return; }
    char *slot = nyrt_map_ent(m, i) + m->koff;
    if (m->vt->retain) m->vt->retain((void *)v);   // before the old value goes (`m[k] = m[k]`)
    if (m->vt->release) m->vt->release(slot);
    memcpy(slot, v, (size_t)m->vt->size);
}
static void nyrt_map_remove(nyrt_map **p, const void *k) {
    nyrt_map_unique(p);
    nyrt_map *m = *p;
    int64_t i = nyrt_map_find(m, k);
    if (i < 0) return;
    char *e = nyrt_map_ent(m, i);
    if (m->kt->release) m->kt->release(e);
    if (m->vt->release) m->vt->release(e + m->koff);
    m->alive[i] = false;
    m->len--;
    if (m->n > 8 && 2 * m->len < m->n) {
        // compact: move the live entries together and index them again
        int64_t j = 0;
        for (int64_t k2 = 0; k2 < m->n; k2++) {
            if (!m->alive[k2]) continue;
            if (j != k2) memcpy(nyrt_map_ent(m, j), nyrt_map_ent(m, k2), (size_t)m->esize);
            m->alive[j++] = true;
        }
        m->n = j;
        nyrt_map_reindex(m, m->nslots);
    } else if (!m->len) {
        m->n = 0;
        nyrt_map_reindex(m, m->nslots);
    }
}
static bool nyrt_map_has(const nyrt_map *m, const void *k) { NYRT_LIVE(m); return nyrt_map_find(m, k) >= 0; }
// The address of `m[k]` (E0247 when the key is missing).
static void *nyrt_map_at(const nyrt_map *m, const void *k, int line, int col) {
    NYRT_LIVE(m);
    int64_t i = nyrt_map_find(m, k);
    if (i < 0) {
        nyrt_buf b = nyrt_buf_new();
        nyrt_buf_cstr(&b, "key ");
        m->kt->fmt(&b, k);
        nyrt_buf_cstr(&b, " is not in the map");
        nyrt_panic("E0247", nyrt_buf_done(&b)->data, "check with `m.has(k)` first, or read it with `m.get(k, default)`", line, col);
    }
    return nyrt_map_ent(m, i) + m->koff;
}
static void *nyrt_map_or(const nyrt_map *m, const void *k, void *d) {
    NYRT_LIVE(m);
    int64_t i = nyrt_map_find(m, k);
    return i < 0 ? d : nyrt_map_ent(m, i) + m->koff;
}
// A new array of the keys (`value` false) or the values, in insertion order.
static nyrt_arr *nyrt_map_list(const nyrt_map *m, bool value) {
    NYRT_LIVE(m);
    const nyrt_type *t = value ? m->vt : m->kt;
    nyrt_arr *r = nyrt_arr_new(t, m->len);
    for (int64_t i = 0; i < m->n; i++)
        if (m->alive[i]) nyrt_arr_push(&r, nyrt_map_ent(m, i) + (value ? m->koff : 0));
    return r;
}
// The same keys with equal values, in any order (no shortcut for the same map: NaN never equals itself).
static bool nyrt_map_eq(const nyrt_map *a, const nyrt_map *b) {
    NYRT_LIVE(a); NYRT_LIVE(b);
    if (a->len != b->len) return false;
    for (int64_t i = 0; i < a->n; i++) {
        if (!a->alive[i]) continue;
        int64_t j = nyrt_map_find(b, nyrt_map_ent(a, i));
        if (j < 0 || !a->vt->eq(nyrt_map_ent(a, i) + a->koff, nyrt_map_ent(b, j) + b->koff)) return false;
    }
    return true;
}
static void nyrt_buf_map(nyrt_buf *b, const nyrt_map *m) {
    NYRT_LIVE(m);
    if (!m->len) { nyrt_buf_add(b, "[:]", 3); return; }
    nyrt_buf_add(b, "[", 1);
    bool first = true;
    for (int64_t i = 0; i < m->n; i++) {
        if (!m->alive[i]) continue;
        if (!first) nyrt_buf_add(b, ", ", 2);
        first = false;
        m->kt->fmt(b, nyrt_map_ent(m, i));
        nyrt_buf_add(b, ": ", 2);
        m->vt->fmt(b, nyrt_map_ent(m, i) + m->koff);
    }
    nyrt_buf_add(b, "]", 1);
}
static void nyrt_put_map(const nyrt_map *m) {
    nyrt_buf b = nyrt_buf_new();
    nyrt_buf_map(&b, m);
    fwrite(b.s->data, 1, (size_t)b.s->len, stdout);
    nyrt_str_release(b.s);
}
static void nyrt_retain_map(void *e) { nyrt_map_retain(*(nyrt_map **)e); }
static void nyrt_release_map(void *e) { nyrt_map_release(*(nyrt_map **)e); }
static void nyrt_keep_map(void *e) { nyrt_map_keep(*(nyrt_map **)e); }
static bool nyrt_eq_map(const void *a, const void *b) { return nyrt_map_eq(*(nyrt_map *const *)a, *(nyrt_map *const *)b); }
static void nyrt_fmt_map(nyrt_buf *b, const void *e) { nyrt_buf_map(b, *(nyrt_map *const *)e); }
static const nyrt_type nyrt_T_map = {
    sizeof(nyrt_map *), nyrt_retain_map, nyrt_release_map, nyrt_eq_map, nyrt_fmt_map, NULL, nyrt_keep_map
};
