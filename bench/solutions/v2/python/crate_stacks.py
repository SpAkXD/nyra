import sys

lines = sys.stdin.read().splitlines()
split = lines.index("")
drawing = lines[:split]
stack_count = len(drawing[-1].split())
base = [[] for _ in range(stack_count)]
for row in reversed(drawing[:-1]):
    for k in range(stack_count):
        if 1 + 4 * k < len(row) and row[1 + 4 * k] != " ":
            base[k].append(row[1 + 4 * k])
one = [list(s) for s in base]
many = [list(s) for s in base]
for number, line in enumerate(lines, 1):
    if number <= split + 1 or line == "":
        continue
    words = line.split(" ")
    count, src, dst = int(words[1]), int(words[3]), int(words[5])
    if not (1 <= src <= stack_count and 1 <= dst <= stack_count) or count > len(one[src - 1]):
        print(f"line {number}: impossible move")
        continue
    for _ in range(count):
        one[dst - 1].append(one[src - 1].pop())
    lifted = many[src - 1][len(many[src - 1]) - count:]
    del many[src - 1][len(many[src - 1]) - count:]
    many[dst - 1].extend(lifted)
print("one at a time: " + "".join(s[-1] if s else "-" for s in one))
print("all at once: " + "".join(s[-1] if s else "-" for s in many))
