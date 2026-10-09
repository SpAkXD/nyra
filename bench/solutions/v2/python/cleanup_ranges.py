import re
import sys

contained = overlapping = 0
for number, line in enumerate(sys.stdin.read().splitlines(), 1):
    if line == "":
        continue
    m = re.fullmatch(r"([0-9]+)-([0-9]+),([0-9]+)-([0-9]+)", line)
    if not m:
        print(f"line {number}: invalid")
        continue
    a, b, c, d = (int(g) for g in m.groups())
    if a > b or c > d:
        print(f"line {number}: invalid")
        continue
    if (a <= c and d <= b) or (c <= a and b <= d):
        contained += 1
    if a <= d and c <= b:
        overlapping += 1
print(f"contained={contained} overlapping={overlapping}")
