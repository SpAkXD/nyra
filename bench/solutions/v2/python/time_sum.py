import re
import sys

PATTERN = re.compile(r"([0-9]{1,4}):([0-9]{2}):([0-9]{2})")
total = 0
valid = 0
for number, raw in enumerate(sys.stdin.read().splitlines(), 1):
    s = raw.strip(" ")
    if s == "":
        continue
    m = PATTERN.fullmatch(s)
    if m and int(m.group(2)) < 60 and int(m.group(3)) < 60:
        total += int(m.group(1)) * 3600 + int(m.group(2)) * 60 + int(m.group(3))
        valid += 1
    else:
        print(f"line {number}: invalid")
print(f"total={total // 3600:02d}:{total % 3600 // 60:02d}:{total % 60:02d} valid={valid}")
