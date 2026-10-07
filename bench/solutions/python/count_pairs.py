count = 0
for a in range(1, 51):
    for b in range(a + 1, 51):
        if (a + b) % 5 == 0:
            count += 1
print(count)
