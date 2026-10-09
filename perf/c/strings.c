// string building, split, join, char scans (reference for strings.nyra)
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct { char *p; size_t len, cap; } buf;
static void add(buf *b, const char *s, size_t n) {
    if (b->len + n + 1 > b->cap) {
        b->cap = (b->len + n + 1) * 2;
        b->p = realloc(b->p, b->cap);
    }
    memcpy(b->p + b->len, s, n);
    b->len += n;
    b->p[b->len] = 0;
}

int main(void) {
    buf s = {0};
    char t[64];
    for (int64_t i = 0; i < 2000000; i++) {
        add(&s, t, (size_t)snprintf(t, sizeof t, "%lld", (long long)i));
        add(&s, ",", 1);
    }
    printf("%zu\n", s.len);
    int64_t sevens = 0;
    for (size_t k = 0; k < s.len; k++) sevens += s.p[k] == '7';
    printf("%lld\n", (long long)sevens);
    // split into separate strings, then parse each
    size_t nparts = 0, cap = 16;
    char **parts = malloc(cap * sizeof *parts);
    size_t from = 0;
    for (size_t k = 0; k <= s.len; k++) {
        if (k == s.len || s.p[k] == ',') {
            if (nparts == cap) parts = realloc(parts, (cap *= 2) * sizeof *parts);
            char *q = malloc(k - from + 1);
            memcpy(q, s.p + from, k - from);
            q[k - from] = 0;
            parts[nparts++] = q;
            from = k + 1;
        }
    }
    int64_t total = 0;
    for (size_t k = 0; k < nparts; k++) {
        if (parts[k][0]) total += strtoll(parts[k], NULL, 10);
        free(parts[k]);
    }
    free(parts);
    printf("%lld\n", (long long)total);
    char **lines = malloc(500000 * sizeof *lines);
    for (int64_t i = 0; i < 500000; i++) {
        int n = snprintf(t, sizeof t, "item %lld: %lld of %lld", (long long)i, (long long)(i * 3), (long long)(i % 7));
        lines[i] = malloc((size_t)n + 1);
        memcpy(lines[i], t, (size_t)n + 1);
    }
    buf text = {0};
    for (int64_t i = 0; i < 500000; i++) {
        if (i) add(&text, "\n", 1);
        add(&text, lines[i], strlen(lines[i]));
        free(lines[i]);
    }
    free(lines);
    printf("%zu\n", text.len);
    free(s.p);
    free(text.p);
    return 0;
}
