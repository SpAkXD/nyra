import re
import sys


def score(tokens):
    if not all(re.fullmatch(r"0|[1-9][0-9]?", t) for t in tokens):
        return None
    r = [int(t) for t in tokens]
    if any(v > 10 for v in r):
        return None
    total = 0
    i = 0
    for _ in range(9):
        if i >= len(r):
            return None
        if r[i] == 10:
            if i + 2 >= len(r):
                return None
            total += 10 + r[i + 1] + r[i + 2]
            i += 1
        else:
            if i + 1 >= len(r) or r[i] + r[i + 1] > 10:
                return None
            total += r[i] + r[i + 1]
            if r[i] + r[i + 1] == 10:
                if i + 2 >= len(r):
                    return None
                total += r[i + 2]
            i += 2
    rest = r[i:]
    if len(rest) < 2:
        return None
    a, b = rest[0], rest[1]
    if a == 10:
        if len(rest) != 3 or (b != 10 and b + rest[2] > 10):
            return None
        return total + 10 + b + rest[2]
    if a + b > 10:
        return None
    if a + b == 10:
        return total + 10 + rest[2] if len(rest) == 3 else None
    return total + a + b if len(rest) == 2 else None


for line in sys.stdin.read().splitlines():
    if line.strip(" ") == "":
        continue
    result = score(line.split(" "))
    print("invalid" if result is None else result)
