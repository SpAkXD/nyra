import sys

lines = sys.stdin.read().splitlines()
rows, cols = (int(t) for t in lines[0].split(" "))
grid = lines[1:1 + rows]
MOD = 1_000_000_007
ways = [[0] * cols for _ in range(rows)]
for r in range(rows):
    for c in range(cols):
        if grid[r][c] == "#":
            continue
        if r == 0 and c == 0:
            ways[r][c] = 1
        else:
            up = ways[r - 1][c] if r > 0 else 0
            left = ways[r][c - 1] if c > 0 else 0
            ways[r][c] = (up + left) % MOD
print(ways[rows - 1][cols - 1])
