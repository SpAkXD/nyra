for row in range(6):
    print("".join("#" if (row + col) % 2 == 0 else "." for col in range(6)))
