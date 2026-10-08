commands = ("add rent 1 900; add food 1 250; add food 2 -40; move rent 1 food 2 100; move food 2 rent 3 200; "
            "add fun 12 75; clear fun 12; clear fun 12; add rent 3 1300; move rent 3 rent 3 50; add zoo 5 0; "
            "add tax 4 -1300; move rent 3 tax 4 1300; add food 1 -250; add car 7 15; move car 7 car 8 15; "
            "add tax 10 -75; add food 10 12000; move food 10 rent 10 2500")

cells = {}


def get(name, month):
    return cells.get((name, month), 0)


def put(name, month, value):
    if value == 0:
        cells.pop((name, month), None)
    else:
        cells[(name, month)] = value


for command in commands.split("; "):
    w = command.split(" ")
    if w[0] == "add":
        name, month = w[1], int(w[2])
        put(name, month, get(name, month) + int(w[3]))
        print(f"{name} {month} = {get(name, month)}")
    elif w[0] == "move":
        a, ma, b, mb, n = w[1], int(w[2]), w[3], int(w[4]), int(w[5])
        if get(a, ma) < n:
            print("move rejected")
            continue
        put(a, ma, get(a, ma) - n)
        put(b, mb, get(b, mb) + n)
        print(f"{a} {ma} = {get(a, ma)}, {b} {mb} = {get(b, mb)}")
    else:
        name, month = w[1], int(w[2])
        if get(name, month) == 0:
            print(f"{name} {month} already empty")
        else:
            print(f"{name} {month} cleared (was {get(name, month)})")
            put(name, month, 0)

accounts = sorted({name for name, _ in cells})
months = sorted({month for _, month in cells})
table = [["account"] + [str(m) for m in months] + ["total"]]
for name in accounts:
    row = [name] + [str(get(name, m)) if get(name, m) else "-" for m in months]
    table.append(row + [str(sum(get(name, m) for m in months))])
table.append(["total"] + [str(sum(get(a, m) for a in accounts)) for m in months] + [str(sum(cells.values()))])
widths = [max(len(row[i]) for row in table) for i in range(len(table[0]))]
for row in table:
    print("  ".join([row[0].ljust(widths[0])] + [cell.rjust(w) for cell, w in zip(row[1:], widths[1:])]))
