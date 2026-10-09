def gen(x):
    s = []
    for _ in range(1000):
        x = (x * 75 + 74) % 65537
        s.append(x % 4)
    return s

A = gen(1)
B = gen(2)

n = 1000
full = (1 << n) - 1
M = [0] * 4
for j, c in enumerate(B):
    M[c] |= 1 << j

ks = {250, 500, 750, 1000}
V = full
res = {}
for i, a in enumerate(A, 1):
    U = V & M[a]
    V = ((V + U) | (V - U)) & full
    if i in ks:
        low = V & ((1 << i) - 1)
        res[i] = i - bin(low).count("1")

for k in (250, 500, 750, 1000):
    print(res[k])
