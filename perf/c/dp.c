// knapsack (one array) and LCS (a 2D table) (reference for dp.nyra)
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

static int64_t knapsack(const int64_t *w, const int64_t *v, int64_t n, int64_t cap) {
    int64_t *best = calloc((size_t)cap + 1, sizeof *best);
    for (int64_t k = 0; k < n; k++)
        for (int64_t c = cap; c >= w[k]; c--) {
            int64_t take = best[c - w[k]] + v[k];
            if (take > best[c]) best[c] = take;
        }
    int64_t r = best[cap];
    free(best);
    return r;
}

static int64_t lcs(const int64_t *a, int64_t n, const int64_t *b, int64_t m) {
    int64_t **dp = malloc((size_t)(n + 1) * sizeof *dp);
    for (int64_t i = 0; i <= n; i++) dp[i] = calloc((size_t)m + 1, sizeof **dp);
    for (int64_t i = 1; i <= n; i++)
        for (int64_t j = 1; j <= m; j++) {
            if (a[i - 1] == b[j - 1]) dp[i][j] = dp[i - 1][j - 1] + 1;
            else dp[i][j] = dp[i - 1][j] > dp[i][j - 1] ? dp[i - 1][j] : dp[i][j - 1];
        }
    int64_t r = dp[n][m];
    for (int64_t i = 0; i <= n; i++) free(dp[i]);
    free(dp);
    return r;
}

static int64_t seed = 42;
static int64_t next(void) { return seed = (seed * 1103515245 + 12345) % 2147483648; }

int main(void) {
    int64_t w[1000], v[1000];
    for (int k = 0; k < 1000; k++) {
        w[k] = (next() / 65536) % 1000 + 1;
        v[k] = (next() / 65536) % 1000 + 1;
    }
    printf("%lld\n", (long long)knapsack(w, v, 1000, 50000));
    static int64_t a[2500], b[2500];
    for (int k = 0; k < 2500; k++) {
        a[k] = (next() / 65536) % 4;
        b[k] = (next() / 65536) % 4;
    }
    printf("%lld\n", (long long)lcs(a, 2500, b, 2500));
    return 0;
}
