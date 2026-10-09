import re
import sys

totals = []
current = None
for number, line in enumerate(sys.stdin.read().splitlines(), 1):
    if line == "":
        if current is not None:
            totals.append(current)
            current = None
    elif re.fullmatch(r"[0-9]+", line):
        current = (current or 0) + int(line)
    else:
        print(f"line {number}: invalid")
if current is not None:
    totals.append(current)
totals.sort(reverse=True)
print(f"hikers={len(totals)}")
print(f"top1={totals[0] if totals else 0}")
print(f"top3={sum(totals[:3])}")
