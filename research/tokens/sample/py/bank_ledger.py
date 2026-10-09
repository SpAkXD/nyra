cmds = "open A 100; open B 50; open A 10; deposit C 5; withdraw B 70; transfer A B 60; transfer B A 112; transfer B A 109; transfer A A 5; withdraw A 25; close B; transfer A B 1; deposit A 7; withdraw A 15; close A; open B 0; close B"

acc = {}
for c in cmds.split(";"):
    p = c.split()
    op = p[0]
    if op == "open":
        x, n = p[1], int(p[2])
        if x in acc:
            print(f"{x} exists")
        else:
            acc[x] = n
            print(f"opened {x}")
    elif op == "deposit":
        x, n = p[1], int(p[2])
        if x not in acc:
            print(f"{x} unknown")
        else:
            acc[x] += n
            print(f"{x} balance {acc[x]}")
    elif op == "withdraw":
        x, n = p[1], int(p[2])
        if x not in acc:
            print(f"{x} unknown")
        elif acc[x] - n < 0:
            print(f"{x} insufficient")
        else:
            acc[x] -= n
            print(f"{x} balance {acc[x]}")
    elif op == "transfer":
        x, y, n = p[1], p[2], int(p[3])
        if x not in acc or y not in acc:
            print("unknown account")
        elif acc[x] < n + 1:
            print(f"{x} insufficient")
        else:
            if x == y:
                acc[x] -= 1
            else:
                acc[x] -= n + 1
                acc[y] += n
            print(f"{x} balance {acc[x]} {y} balance {acc[y]}")
    elif op == "close":
        x = p[1]
        if x not in acc:
            print(f"{x} unknown")
        elif acc[x] != 0:
            print(f"{x} not empty")
        else:
            del acc[x]
            print(f"closed {x}")
