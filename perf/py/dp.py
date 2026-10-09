# knapsack (one array) and LCS (a 2D table) (reference for dp.nyra)
seed = 42


def nxt():
    global seed
    seed = (seed * 1103515245 + 12345) % 2147483648
    return seed


def knapsack(w, v, cap):
    best = [0] * (cap + 1)
    for k in range(len(w)):
        wk, vk = w[k], v[k]
        for c in range(cap, wk - 1, -1):
            take = best[c - wk] + vk
            if take > best[c]:
                best[c] = take
    return best[cap]


def lcs(a, b):
    n, m = len(a), len(b)
    dp = [[0] * (m + 1) for _ in range(n + 1)]
    for i in range(1, n + 1):
        row, prev, ai = dp[i], dp[i - 1], a[i - 1]
        for j in range(1, m + 1):
            if ai == b[j - 1]:
                row[j] = prev[j - 1] + 1
            else:
                row[j] = max(prev[j], row[j - 1])
    return dp[n][m]


w, v = [], []
for _ in range(1000):
    w.append((nxt() // 65536) % 1000 + 1)
    v.append((nxt() // 65536) % 1000 + 1)
print(knapsack(w, v, 50000))
a, b = [], []
for _ in range(2500):
    a.append((nxt() // 65536) % 4)
    b.append((nxt() // 65536) % 4)
print(lcs(a, b))
