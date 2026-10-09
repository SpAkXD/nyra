# string building, split, join, char scans (reference for strings.nyra)
parts_out = []
for i in range(2000000):
    parts_out.append(str(i))
    parts_out.append(",")
s = "".join(parts_out)
print(len(s))
print(s.count("7"))
total = 0
for p in s.split(","):
    if p:
        total += int(p)
print(total)
lines = [f"item {i}: {i * 3} of {i % 7}" for i in range(500000)]
print(len("\n".join(lines)))
