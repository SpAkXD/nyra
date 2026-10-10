import sys

steps = int(sys.stdin.read().split()[0])
black = set()
x = y = 0
facing = 0  # 0 up, 1 right, 2 down, 3 left
dx = [0, 1, 0, -1]
dy = [-1, 0, 1, 0]
for _ in range(steps):
    if (x, y) in black:
        facing = (facing + 3) % 4
        black.remove((x, y))
    else:
        facing = (facing + 1) % 4
        black.add((x, y))
    x += dx[facing]
    y += dy[facing]
print(f"black={len(black)} x={x} y={y} facing={'URDL'[facing]}")
