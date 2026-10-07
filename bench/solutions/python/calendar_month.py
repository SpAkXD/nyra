days = 31
first = 2  # Monday is 0, so Wednesday is 2

print("Mo Tu We Th Fr Sa Su")
cells = ["  "] * first + [f"{day:>2}" for day in range(1, days + 1)]
for start in range(0, len(cells), 7):
    print(" ".join(cells[start:start + 7]).rstrip())
