import sys

for line in sys.stdin.read().splitlines():
    if line == "":
        print("(empty)")
        continue
    best_start, best_len = 0, 1
    for center in range(len(line)):
        for lo, hi in ((center, center), (center, center + 1)):
            while lo >= 0 and hi < len(line) and line[lo] == line[hi]:
                lo -= 1
                hi += 1
            length = hi - lo - 1
            start = lo + 1
            if length > best_len or (length == best_len and start < best_start):
                best_start, best_len = start, length
    print(line[best_start:best_start + best_len])
