import sys

tokens = sys.stdin.read().split()
n, capacity = int(tokens[0]), int(tokens[1])
items = [(int(tokens[2 + 2 * i]), int(tokens[3 + 2 * i])) for i in range(n)]
INF = float("inf")
# best[w] = (largest value, fewest items for that value) using total weight at most w
best = [(0, 0)] * (capacity + 1)
for weight, value in items:
    for w in range(capacity, weight - 1, -1):
        v, k = best[w - weight]
        candidate = (v + value, k + 1)
        if candidate[0] > best[w][0] or (candidate[0] == best[w][0] and candidate[1] < best[w][1]):
            best[w] = candidate
print(f"best={best[capacity][0]}")
print(f"taken={best[capacity][1]}")
