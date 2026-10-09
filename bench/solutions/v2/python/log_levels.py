import re
import sys

PATTERN = re.compile(r"[0-9]{4}-([0-9]{2})-([0-9]{2}) ([0-9]{2}):([0-9]{2}):([0-9]{2}) (DEBUG|INFO|WARN|ERROR) (.+)", re.S)
counts = {"DEBUG": 0, "INFO": 0, "WARN": 0, "ERROR": 0}
last = "none"
for number, line in enumerate(sys.stdin.read().splitlines(), 1):
    if line == "":
        continue
    m = PATTERN.fullmatch(line)
    if m is not None:
        month, day, hour, minute, second = (int(m.group(i)) for i in range(1, 6))
        if 1 <= month <= 12 and 1 <= day <= 31 and hour <= 23 and minute <= 59 and second <= 59:
            counts[m.group(6)] += 1
            if m.group(6) == "ERROR":
                last = m.group(7)
            continue
    print(f"line {number}: malformed")
for level in ("DEBUG", "INFO", "WARN", "ERROR"):
    print(f"{level}: {counts[level]}")
print(f"last error: {last}")
