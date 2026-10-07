def binom(n, k):
    if k == 0 or k == n:
        return 1
    return binom(n - 1, k - 1) + binom(n - 1, k)


for n, k in ((5, 2), (10, 5), (20, 10)):
    print(binom(n, k))
