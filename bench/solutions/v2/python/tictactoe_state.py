import sys

LINES = [(0, 1, 2), (3, 4, 5), (6, 7, 8), (0, 3, 6), (1, 4, 7), (2, 5, 8), (0, 4, 8), (2, 4, 6)]


def state(s):
    if len(s) != 9 or any(c not in "XO." for c in s):
        return "invalid"
    x, o = s.count("X"), s.count("O")
    if not (x == o or x == o + 1):
        return "invalid"
    xw = any(all(s[i] == "X" for i in line) for line in LINES)
    ow = any(all(s[i] == "O" for i in line) for line in LINES)
    if xw and ow:
        return "invalid"
    if xw:
        return "X wins" if x == o + 1 else "invalid"
    if ow:
        return "O wins" if x == o else "invalid"
    return "draw" if "." not in s else "in progress"


for line in sys.stdin.read().splitlines():
    if line != "":
        print(state(line))
