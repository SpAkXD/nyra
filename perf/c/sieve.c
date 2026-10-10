// sieve of Eratosthenes to 10,000,000, ten rounds (reference for sieve.nyra)
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int64_t count_primes(int64_t n) {
    bool *sieve = malloc((size_t)n + 1);
    memset(sieve, 1, (size_t)n + 1);
    sieve[0] = sieve[1] = false;
    for (int64_t i = 2; i * i <= n; i++)
        if (sieve[i])
            for (int64_t j = i * i; j <= n; j += i) sieve[j] = false;
    int64_t count = 0;
    for (int64_t k = 0; k <= n; k++) count += sieve[k];
    free(sieve);
    return count;
}

int main(void) {
    int64_t total = 0;
    for (int round = 0; round < 10; round++) total += count_primes(10000000 - round);
    printf("%lld\n", (long long)total);
    return 0;
}
