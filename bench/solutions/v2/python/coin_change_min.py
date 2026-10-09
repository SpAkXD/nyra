import sys

lines = sys.stdin.read().splitlines()
coins = sorted((int(t) for t in lines[0].split(" ")), reverse=True)
for line in lines[1:]:
    target = int(line)
    INF = float("inf")
    fewest = [0] + [INF] * target
    for amount in range(1, target + 1):
        for c in coins:
            if c <= amount and fewest[amount - c] + 1 < fewest[amount]:
                fewest[amount] = fewest[amount - c] + 1
    if fewest[target] == INF:
        print("impossible")
        continue
    used = []
    left = target
    while left > 0:
        # the largest coin that still allows the fewest total: the first in largest-first order
        for c in coins:
            if c <= left and fewest[left - c] == fewest[left] - 1:
                used.append(c)
                left -= c
                break
    print(f"{fewest[target]}:" + "".join(f" {c}" for c in used))
