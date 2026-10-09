import sys

for raw in sys.stdin.read().splitlines():
    if raw.strip(" ") == "":
        continue
    s = raw.replace(" ", "").replace("-", "")
    if not (13 <= len(s) <= 19) or not all("0" <= c <= "9" for c in s):
        print("bad format")
        continue
    total = 0
    for i, c in enumerate(reversed(s)):
        d = int(c)
        if i % 2 == 1:
            d *= 2
            if d > 9:
                d -= 9
        total += d
    print("valid" if total % 10 == 0 else "bad checksum")
