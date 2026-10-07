n = 6
parts = [str(n)]
while n != 1:
    n = n // 2 if n % 2 == 0 else 3 * n + 1
    parts.append(str(n))
print(" -> ".join(parts))
