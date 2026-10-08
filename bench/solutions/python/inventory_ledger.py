commands = ("receive bolt 12; reserve bolt 9; reserve nut 1; receive nut 4; reserve bolt 4; receive bolt 10; "
            "ship bolt 11; release bolt 1; reserve bolt 7; count bolt 6; count bolt 15; release bolt 7; ship nut 4; "
            "ship nut 1; receive washer 30; reserve washer 25; ship washer 1; count washer 26; count nut 0; "
            "ship bolt 15; receive bolt 5; release gear 2")

items = {}  # name -> [on_hand, reserved]


def available(name):
    on_hand, reserved = items[name]
    return on_hand - reserved


for command in commands.split("; "):
    op, name, amount = command.split(" ")
    n = int(amount)
    if op == "receive" and name not in items:
        items[name] = [0, 0]
    if name not in items:
        print(f"{name} unknown")
        continue
    before = available(name)
    item = items[name]
    ok = True
    if op == "receive":
        item[0] += n
        print(f"{name} on-hand {item[0]} available {available(name)}")
    elif op == "reserve":
        if n > available(name):
            print(f"{name} reserve rejected (available {available(name)})")
            ok = False
        else:
            item[1] += n
            print(f"{name} reserved {item[1]} available {available(name)}")
    elif op == "release":
        if n > item[1]:
            print(f"{name} release rejected (reserved {item[1]})")
            ok = False
        else:
            item[1] -= n
            print(f"{name} reserved {item[1]} available {available(name)}")
    elif op == "ship":
        if n > item[0]:
            print(f"{name} ship rejected (on-hand {item[0]})")
            ok = False
        else:
            item[0] -= n
            item[1] = max(0, item[1] - n)
            print(f"{name} shipped {n} on-hand {item[0]} reserved {item[1]}")
    elif op == "count":
        if n < item[1]:
            print(f"{name} count rejected (reserved {item[1]})")
            ok = False
        else:
            diff = n - item[0]
            item[0] = n
            sign = "+" if diff > 0 else ""
            print(f"{name} adjusted by {sign}{diff}")
    if ok and available(name) < 5 <= before:
        print(f"{name} low stock")

print("---")
for name in sorted(items):
    on_hand, reserved = items[name]
    print(f"{name}: on-hand {on_hand}, reserved {reserved}, available {on_hand - reserved}")
