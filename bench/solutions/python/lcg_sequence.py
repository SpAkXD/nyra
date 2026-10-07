x = 1
for _ in range(10):
    x = (x * 75 + 74) % 65537
    print(x)
