# sieve of Eratosthenes to 10,000,000, ten rounds (reference for sieve.nyra)
def count_primes(n):
    sieve = bytearray([1]) * (n + 1)
    sieve[0] = sieve[1] = 0
    i = 2
    while i * i <= n:
        if sieve[i]:
            sieve[i * i :: i] = bytes(len(range(i * i, n + 1, i)))
        i += 1
    return sum(sieve)


total = 0
for rnd in range(10):
    total += count_primes(10000000 - rnd)
print(total)
