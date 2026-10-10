def run(moves):
    R, C = 5, 6
    board = [['.'] * C for _ in range(R)]
    heights = [0] * C
    for i, m in enumerate(moves):
        n = i + 1
        p = 'X' if i % 2 == 0 else 'O'
        if m < 1 or m > C or heights[m - 1] >= R:
            return "illegal move %d by %s" % (n, p), board
        c = m - 1
        r = heights[c]
        board[r][c] = p
        heights[c] += 1
        for dr, dc in ((0, 1), (1, 0), (1, 1), (1, -1)):
            cnt = 1
            for s in (1, -1):
                rr, cc = r + dr * s, c + dc * s
                while 0 <= rr < R and 0 <= cc < C and board[rr][cc] == p:
                    cnt += 1
                    rr += dr * s
                    cc += dc * s
            if cnt >= 4:
                return "%s wins at move %d" % (p, n), board
    if all(h == R for h in heights):
        return "draw", board
    return "no winner after %d moves" % len(moves), board

games = [
    [3, 4, 3, 4, 3, 4, 3],
    [1, 2, 2, 3, 3, 4, 3, 4, 4, 6, 4],
    [6, 6, 6, 6, 6, 6],
    [2, 3, 4, 5, 1, 2, 3, 4, 7],
    [1, 1, 2, 2, 4, 4, 3, 5, 5],
    [4, 3, 3, 2, 2, 1, 2, 1, 1, 5, 1],
    [1, 2, 3, 4, 5, 6, 6, 5],
]

for k, g in enumerate(games, 1):
    res, board = run(g)
    print("game %d: %s" % (k, res))
    for r in range(4, -1, -1):
        print(''.join(board[r]))
