def factorization(n):
    parts = []
    rest = n
    p = 2
    while p * p <= rest:
        exponent = 0
        while rest % p == 0:
            rest //= p
            exponent += 1
        if exponent == 1:
            parts.append(str(p))
        elif exponent > 1:
            parts.append(f"{p}^{exponent}")
        p += 1
    if rest > 1:
        parts.append(str(rest))
    return f"{n} = " + " * ".join(parts)


for n in (360, 97, 1001, 65536, 999999):
    print(factorization(n))
