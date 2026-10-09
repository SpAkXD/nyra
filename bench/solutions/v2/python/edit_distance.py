import sys


def distance(a, b):
    previous = list(range(len(b) + 1))
    for i in range(1, len(a) + 1):
        row = [i]
        for j in range(1, len(b) + 1):
            row.append(min(previous[j] + 1, row[j - 1] + 1, previous[j - 1] + (a[i - 1] != b[j - 1])))
        previous = row
    return previous[len(b)]


for line in sys.stdin.read().splitlines():
    parts = line.split("|")
    print(distance(parts[0], parts[1]) if len(parts) == 2 else "invalid")
