start = ["#......#", "..##....", ".#..#...", "..##....", "........", "#.....##"]
GENERATIONS = 7
rows, cols = len(start), len(start[0])
age = [[1 if c == "#" else 0 for c in row] for row in start]

for gen in range(1, GENERATIONS + 1):
    new = [[0] * cols for _ in range(rows)]
    for r in range(rows):
        for c in range(cols):
            live = 0
            for dr in (-1, 0, 1):
                for dc in (-1, 0, 1):
                    if (dr or dc) and age[(r + dr) % rows][(c + dc) % cols] > 0:
                        live += 1
            if age[r][c] == 0:
                new[r][c] = 1 if live == 3 else 0
            elif live in (2, 3) and age[r][c] + 1 < 5:
                new[r][c] = age[r][c] + 1
    age = new
    alive = sum(1 for row in age for a in row if a > 0)
    oldest = max(a for row in age for a in row)
    print(f"gen {gen}: {alive} alive, oldest {oldest}")
for row in age:
    print("".join(str(a) if a else "." for a in row))
