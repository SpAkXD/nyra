def sequence(seed, n):
    out = []
    x = seed
    for _ in range(n):
        x = (x * 75 + 74) % 65537
        out.append(x % 4)
    return out


N = 1000
a = sequence(1, N)
b = sequence(2, N)
prev = [0] * (N + 1)
for i in range(1, N + 1):
    cur = [0] * (N + 1)
    ai = a[i - 1]
    for j in range(1, N + 1):
        if ai == b[j - 1]:
            cur[j] = prev[j - 1] + 1
        elif prev[j] >= cur[j - 1]:
            cur[j] = prev[j]
        else:
            cur[j] = cur[j - 1]
    prev = cur
    if i % 250 == 0:
        print(prev[i])
