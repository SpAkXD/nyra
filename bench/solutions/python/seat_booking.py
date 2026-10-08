ROWS = "ABCDEF"
PREFERENCE = "CDBEAF"
SEATS = 10
broken = {("C", 5), ("C", 6), ("E", 1)}
requests = ("ann 4; bob 4; cat 2; dan 10; eve 1; fay 6; cancel bob; gus 5; hal 3; cancel zed; ivy 9; jo 2; "
            "kai 8; lea 3")

owner = {}  # (row, seat) -> group


def free(row, seat):
    return (row, seat) not in broken and (row, seat) not in owner


for request in requests.split("; "):
    words = request.split(" ")
    if words[0] == "cancel":
        name = words[1]
        seats = [s for s, g in owner.items() if g == name]
        if not seats:
            print(f"{name} has no booking")
        else:
            for s in seats:
                del owner[s]
            print(f"{name} cancelled ({len(seats)} seats)")
        continue
    name, k = words[0], int(words[1])
    choice = None
    for row in PREFERENCE:
        best = None
        for start in range(1, SEATS - k + 2):
            if all(free(row, s) for s in range(start, start + k)):
                # twice the distance from the block's middle to the row's middle (between seats 5 and 6)
                distance = abs(2 * start + k - 1 - 11)
                if best is None or distance < best[0]:
                    best = (distance, start)
        if best is not None:
            choice = (row, best[1])
            break
    if choice is None:
        print(f"{name}: no room")
        continue
    row, start = choice
    for s in range(start, start + k):
        owner[(row, s)] = name
    print(f"{name}: {row}{start}" if k == 1 else f"{name}: {row}{start}-{row}{start + k - 1}")

for row in ROWS:
    line = ""
    for s in range(1, SEATS + 1):
        line += "x" if (row, s) in broken else "#" if (row, s) in owner else "."
    print(f"{row} {line}")
