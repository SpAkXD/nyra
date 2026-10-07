for c in range(1, 51):
    for a in range(1, c):
        for b in range(a + 1, c):
            if a * a + b * b == c * c:
                print(a, b, c)
