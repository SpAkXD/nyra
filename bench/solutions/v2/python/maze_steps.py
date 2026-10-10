import sys
from collections import deque

blocks = []
current = []
for line in sys.stdin.read().splitlines():
    if line == "":
        if current:
            blocks.append(current)
            current = []
    else:
        current.append(line)
if current:
    blocks.append(current)

for k, grid in enumerate(blocks, 1):
    h, w = len(grid), len(grid[0])
    start = end = None
    for r in range(h):
        for c in range(w):
            if grid[r][c] == "S":
                start = (r, c)
            elif grid[r][c] == "E":
                end = (r, c)
    dist = {start: 0}
    queue = deque([start])
    while queue:
        r, c = queue.popleft()
        for dr, dc in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            nr, nc = r + dr, c + dc
            if 0 <= nr < h and 0 <= nc < w and grid[nr][nc] != "#" and (nr, nc) not in dist:
                dist[(nr, nc)] = dist[(r, c)] + 1
                queue.append((nr, nc))
    print(f"Maze {k}: {dist[end]}" if end in dist else f"Maze {k}: no path")
