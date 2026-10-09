// recursion: fib(n) for n = 0 to 35, summed (reference for fib.nyra)
#include <stdint.h>
#include <stdio.h>

static int64_t fib(int64_t n) { return n < 2 ? n : fib(n - 1) + fib(n - 2); }

int main(void) {
    int64_t total = 0;
    for (int64_t n = 0; n < 36; n++) total += fib(n);
    printf("%lld\n", (long long)total);
    return 0;
}
