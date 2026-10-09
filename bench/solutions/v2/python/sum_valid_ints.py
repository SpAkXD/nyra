import re
import sys

total = 0
count = 0
for number, raw in enumerate(sys.stdin.read().splitlines(), 1):
    s = raw.strip(" \t")
    if s == "":
        continue
    if re.fullmatch(r"-?[0-9]+", s):
        total += int(s)
        count += 1
    else:
        print(f"line {number}: invalid")
print(f"sum={total} count={count}")
