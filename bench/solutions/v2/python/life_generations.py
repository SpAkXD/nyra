import sys

lines = sys.stdin.read().splitlines()
w, h, generations = (int(t) for t in lines[0].split(" "))
grid = [[c == "#" for c in lines[1 + r]] for r in range(h)]
for _ in range(generations):
    new = [[False] * w for _ in range(h)]
    for r in range(h):
        for c in range(w):
            n = 0
            for dr in (-1, 0, 1):
                for dc in (-1, 0, 1):
                    if (dr or dc) and 0 <= r + dr < h and 0 <= c + dc < w and grid[r + dr][c + dc]:
                        n += 1
            new[r][c] = n == 3 or (grid[r][c] and n == 2)
    grid = new
for row in grid:
    print("".join("#" if cell else "." for cell in row))
print(f"alive={sum(sum(row) for row in grid)}")
