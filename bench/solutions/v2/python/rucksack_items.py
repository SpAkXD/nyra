import sys


def priority(c):
    return ord(c) - 96 if c.islower() else ord(c) - 38


valid = []
shared = 0
for number, line in enumerate(sys.stdin.read().splitlines(), 1):
    if line == "":
        continue
    if len(line) % 2 != 0 or not all("a" <= c <= "z" or "A" <= c <= "Z" for c in line):
        print(f"line {number}: invalid")
        continue
    valid.append(line)
    half = len(line) // 2
    common = set(line[:half]) & set(line[half:])
    shared += min((priority(c) for c in common), default=0)
badges = 0
for i in range(0, len(valid) - len(valid) % 3, 3):
    common = set(valid[i]) & set(valid[i + 1]) & set(valid[i + 2])
    badges += min((priority(c) for c in common), default=0)
print(f"shared={shared}")
print(f"badges={badges}")
