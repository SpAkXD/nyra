WIDTH, HEIGHT = 8, 6
foods = [(4, 2), (2, 2), (6, 4), (0, 5), (7, 0), (3, 3)]
moves = "RRDLURLDDRDLLLLLLURDRR"
STEP = {"U": (0, -1), "D": (0, 1), "L": (-1, 0), "R": (1, 0)}
OPPOSITE = {"U": "D", "D": "U", "L": "R", "R": "L"}

snake = [(2, 2), (1, 2), (0, 2)]  # head first
direction = "R"
next_food = 0
food = None


def place_food():
    global food, next_food
    food = None
    while next_food < len(foods):
        spot = foods[next_food]
        next_food += 1
        if spot not in snake:
            food = spot
            return


place_food()
eaten = 0
ended_at = None
for k, letter in enumerate(moves, 1):
    if letter != OPPOSITE[direction]:
        direction = letter
    dx, dy = STEP[direction]
    head = (snake[0][0] + dx, snake[0][1] + dy)
    growing = head == food
    body = snake if growing else snake[:-1]
    if not (0 <= head[0] < WIDTH and 0 <= head[1] < HEIGHT) or head in body:
        ended_at = k
        break
    snake.insert(0, head)
    if growing:
        eaten += 1
        place_food()
    else:
        snake.pop()

print(f"game over at move {ended_at}" if ended_at else "all moves done")
print(f"score {eaten}")
print(f"length {len(snake)}")
for y in range(HEIGHT):
    row = ""
    for x in range(WIDTH):
        if (x, y) == snake[0]:
            row += "H"
        elif (x, y) in snake:
            row += "o"
        elif (x, y) == food:
            row += "*"
        else:
            row += "."
    print(row)
