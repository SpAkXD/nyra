slots = {"A1": [65, 3], "A2": [100, 1], "B1": [45, 0], "B2": [120, 2]}  # price, stock
coins = {100: 0, 25: 2, 10: 1, 5: 3}
events = ("insert 25; insert 25; insert 3; select A1; insert 25; select A1; insert 100; select B1; select C9; "
          "select A2; select A2; insert 100; insert 25; select B2; insert 100; select A1; insert 100; select A1; "
          "cancel; insert 10; insert 10; insert 10; insert 100; select A1; cancel; select B2; insert 50; cancel")


def make_change(amount):
    """Greedy change from the store. Returns (coins taken {value: count}, unpaid rest)."""
    taken = {}
    for value in (100, 25, 10, 5):
        count = min(amount // value, coins[value])
        if count > 0:
            taken[value] = count
            amount -= count * value
    return taken, amount


def describe(taken):
    return " ".join(f"{v}x{n}" for v, n in taken.items()) or "none"


credit = 0
for event in events.split("; "):
    words = event.split(" ")
    if words[0] == "insert":
        n = int(words[1])
        if n not in coins:
            print(f"rejected {n}")
        else:
            coins[n] += 1
            credit += n
            print(f"credit {credit}")
    elif words[0] == "select":
        s = words[1]
        if s not in slots:
            print(f"no slot {s}")
            continue
        price, stock = slots[s]
        if stock == 0:
            print(f"{s} sold out")
        elif credit < price:
            print(f"{s} costs {price}, insert {price - credit} more")
        else:
            taken, rest = make_change(credit - price)
            if rest > 0:
                print(f"{s} exact change only")
            else:
                for v, k in taken.items():
                    coins[v] -= k
                slots[s][1] -= 1
                credit = 0
                print(f"{s} vended, change {describe(taken)}")
    else:
        taken, rest = make_change(credit)
        for v, k in taken.items():
            coins[v] -= k
        credit = 0
        print(f"returned {describe(taken)}" + (f" (owed {rest})" if rest else ""))

print("stock " + " ".join(f"{s} {slots[s][1]}" for s in slots))
print("coins " + " ".join(f"{v}x{coins[v]}" for v in (100, 25, 10, 5)))
