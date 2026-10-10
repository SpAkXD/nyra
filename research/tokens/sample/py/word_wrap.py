words = "the quick brown fox jumps over the lazy dog and keeps running far away from here".split()
W = 18
lines = []
cur = ""
for w in words:
    if not cur:
        cur = w
    elif len(cur) + 1 + len(w) <= W:
        cur += " " + w
    else:
        lines.append(cur)
        cur = w
if cur:
    lines.append(cur)
for l in lines:
    print(l)
print(f"lines: {len(lines)}")
