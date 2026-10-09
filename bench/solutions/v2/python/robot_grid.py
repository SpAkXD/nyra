import sys

lines = sys.stdin.read().splitlines()
w, h = (int(t) for t in lines[0].split(" "))
grid = lines[1:1 + h]
commands = "".join(lines[1 + h:])
x = y = 0
for row in range(h):
    for col in range(w):
        if grid[row][col] == "R":
            x, y = col, row
dx = [0, 1, 0, -1]
dy = [-1, 0, 1, 0]
facing = 0
blocked = 0
visited = {(x, y)}
for c in commands:
    if c == "L":
        facing = (facing + 3) % 4
    elif c == "R":
        facing = (facing + 1) % 4
    elif c == "F":
        nx, ny = x + dx[facing], y + dy[facing]
        if 0 <= nx < w and 0 <= ny < h and grid[ny][nx] != "#":
            x, y = nx, ny
            visited.add((x, y))
        else:
            blocked += 1
print(f"x={x} y={y} facing={'NESW'[facing]} blocked={blocked} visited={len(visited)}")
