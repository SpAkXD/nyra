def is_prime(n):
    if n < 2:
        return False
    d = 2
    while d * d <= n:
        if n % d == 0:
            return False
        d += 1
    return True


for n in (1, 2, 3, 4, 17, 25, 97, 100, 7919):
    print("true" if is_prime(n) else "false")
