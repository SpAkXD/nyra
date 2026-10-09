// quicksort and a merge sort of 1,000,000 ints (reference for sort.nyra)
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void quicksort(int64_t *xs, int64_t lo, int64_t hi) {
    if (lo >= hi) return;
    int64_t pivot = xs[(lo + hi) / 2], i = lo, j = hi;
    while (i <= j) {
        while (xs[i] < pivot) i++;
        while (xs[j] > pivot) j--;
        if (i <= j) {
            int64_t t = xs[i]; xs[i] = xs[j]; xs[j] = t;
            i++; j--;
        }
    }
    quicksort(xs, lo, j);
    quicksort(xs, i, hi);
}

static void msort(int64_t *a, int64_t *tmp, int64_t lo, int64_t hi) {
    if (hi - lo < 2) return;
    int64_t mid = lo + (hi - lo) / 2;
    msort(a, tmp, lo, mid);
    msort(a, tmp, mid, hi);
    int64_t i = lo, j = mid;
    for (int64_t k = lo; k < hi; k++) tmp[k] = (j < hi && (i >= mid || a[j] < a[i])) ? a[j++] : a[i++];
    memcpy(a + lo, tmp + lo, (size_t)(hi - lo) * sizeof *a);
}

int main(void) {
    int64_t n = 1000000, seed = 7;
    int64_t *xs = malloc((size_t)n * sizeof *xs), *ys = malloc((size_t)n * sizeof *ys), *tmp = malloc((size_t)n * sizeof *tmp);
    for (int64_t k = 0; k < n; k++) {
        seed = (seed * 1103515245 + 12345) % 2147483648;
        xs[k] = (seed / 64) % 1000000;
    }
    memcpy(ys, xs, (size_t)n * sizeof *xs);
    quicksort(xs, 0, n - 1);
    msort(ys, tmp, 0, n);
    printf("%s\n", memcmp(xs, ys, (size_t)n * sizeof *xs) == 0 ? "true" : "false");
    int64_t check = 0;
    for (int64_t i = 0; i < n; i++) check = (check * 31 + xs[i]) % 1000000007;
    printf("%lld\n", (long long)check);
    free(xs); free(ys); free(tmp);
    return 0;
}
