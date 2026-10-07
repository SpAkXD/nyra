text = "the quick brown fox jumps over the lazy dog and keeps running far away from here"
width = 18

lines = []
current = ""
for word in text.split():
    if current == "":
        current = word
    elif len(current) + 1 + len(word) <= width:
        current += " " + word
    else:
        lines.append(current)
        current = word
if current:
    lines.append(current)
for line in lines:
    print(line)
print(f"lines: {len(lines)}")
