ROWS, COLS = 5, 6
games = ["3 4 3 4 3 4 3", "1 2 2 3 3 4 3 4 4 6 4", "6 6 6 6 6 6", "2 3 4 5 1 2 3 4 7", "1 1 2 2 4 4 3 5 5",
         "4 3 3 2 2 1 2 1 1 5 1", "1 2 3 4 5 6 6 5"]


def wins(board, r, c):
    p = board[r][c]
    for dr, dc in ((0, 1), (1, 0), (1, 1), (1, -1)):
        count = 1
        for sign in (1, -1):
            rr, cc = r + sign * dr, c + sign * dc
            while 0 <= rr < ROWS and 0 <= cc < COLS and board[rr][cc] == p:
                count += 1
                rr += sign * dr
                cc += sign * dc
        if count >= 4:
            return True
    return False


for k, game in enumerate(games, 1):
    board = [["."] * COLS for _ in range(ROWS)]
    result = None
    moves = [int(m) for m in game.split(" ")]
    for n, col in enumerate(moves, 1):
        player = "X" if n % 2 == 1 else "O"
        if not 1 <= col <= COLS or board[0][col - 1] != ".":
            result = f"illegal move {n} by {player}"
            break
        r = ROWS - 1
        while board[r][col - 1] != ".":
            r -= 1
        board[r][col - 1] = player
        if wins(board, r, col - 1):
            result = f"{player} wins at move {n}"
            break
    if result is None:
        full = all(board[0][c] != "." for c in range(COLS))
        result = "draw" if full else f"no winner after {len(moves)} moves"
    print(f"game {k}: {result}")
    for row in board:
        print("".join(row))
