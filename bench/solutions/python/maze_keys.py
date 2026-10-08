mazes = [
    ["S...#",
     ".##.#",
     "....G"],
    ["#########",
     "#S..A..G#",
     "#.#####.#",
     "#...a.#.#",
     "#########"],
    ["S.B.G",
     "#.###",
     "b.A.a"],
    ["#######",
     "#a#.#b#",
     "#.#.#.#",
     "#..S..#",
     "###A###",
     "###B###",
     "###G###"],
    ["S.#..",
     ".##.#",
     "...#G"],
    ["..S..",
     ".###.",
     "..G.."],
]
MOVES = [("D", 1, 0), ("L", 0, -1), ("R", 0, 1), ("U", -1, 0)]  # alphabetical order


def solve(grid):
    rows, cols = len(grid), len(grid[0])
    start = next((r, c) for r in range(rows) for c in range(cols) if grid[r][c] == "S")
    first = (start[0], start[1], frozenset())
    seen = {first}
    layer = [(first, "")]
    while layer:
        layer.sort(key=lambda item: item[1])
        nxt = []
        for (r, c, keys), path in layer:
            for letter, dr, dc in MOVES:
                nr, nc = r + dr, c + dc
                if not (0 <= nr < rows and 0 <= nc < cols):
                    continue
                cell = grid[nr][nc]
                if cell == "#" or (cell.isupper() and cell not in "SG" and cell.lower() not in keys):
                    continue
                if cell == "G":
                    return path + letter
                new_keys = keys | {cell} if cell.islower() else keys
                state = (nr, nc, new_keys)
                if state not in seen:
                    seen.add(state)
                    nxt.append((state, path + letter))
        layer = nxt
    return None


for k, grid in enumerate(mazes, 1):
    path = solve(grid)
    print(f"maze {k}: no path" if path is None else f"maze {k}: {len(path)} moves {path}")
