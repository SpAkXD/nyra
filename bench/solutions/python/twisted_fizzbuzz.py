for n in range(1, 41):
    if "7" in str(n):
        print("Seven")
    elif n % 3 == 0 and n % 4 == 0:
        print("Twelve")
    elif n % 3 == 0:
        print("Three")
    elif n % 4 == 0:
        print("Four")
    else:
        print(n)
