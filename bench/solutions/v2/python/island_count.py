import sys

grid = sys.stdin.read().splitlines()
h = len(grid)
w = len(grid[0]) if h else 0
seen = [[False] * w for _ in range(h)]
islands = 0
largest = 0
for r in range(h):
    for c in range(w):
        if grid[r][c] == "#" and not seen[r][c]:
            islands += 1
            size = 0
            stack = [(r, c)]
            seen[r][c] = True
            while stack:
                y, x = stack.pop()
                size += 1
                for dy in (-1, 0, 1):
                    for dx in (-1, 0, 1):
                        ny, nx = y + dy, x + dx
                        if 0 <= ny < h and 0 <= nx < w and grid[ny][nx] == "#" and not seen[ny][nx]:
                            seen[ny][nx] = True
                            stack.append((ny, nx))
            largest = max(largest, size)
print(f"islands={islands} largest={largest}")
