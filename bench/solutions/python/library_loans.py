BOOKS = ["B1", "B2", "B3"]
MEMBERS = ["ann", "bob", "cat"]
events = ("1 borrow ann B1; 1 borrow bob B1; 2 hold bob B1; 2 hold cat B1; 3 hold bob B1; 3 hold ann B1; "
          "4 hold cat B2; 5 borrow ann B2; 6 borrow ann B3; 6 borrow bob B3; 15 return ann B1; 16 borrow ann B1; "
          "17 borrow bob B1; 18 hold ann B1; 20 return ann B2; 45 return bob B3; 46 borrow bob B2; 47 pay bob 250; "
          "47 borrow bob B2; 48 return cat B1; 50 return bob B1; 51 hold cat B1; 52 borrow ann B1; "
          "53 borrow cat B1; 54 pay ann 100; 55 hold ann B1; 56 return cat B1; 57 borrow ann B1; 58 pay bob 700")

balance = {m: 0 for m in MEMBERS}
loans = {}  # book -> (member, due day)
waiting = {b: [] for b in BOOKS}
held = {}  # book -> member


def money(c):
    return f"{c // 100}.{c % 100:02d}"


def count_loans(member):
    return sum(1 for who, _ in loans.values() if who == member)


for event in events.split("; "):
    day_text, kind, m, arg = event.split(" ")
    day = int(day_text)
    if kind == "borrow":
        b = arg
        if balance[m] >= 300:
            print(f"{m} blocked (owes {money(balance[m])})")
        elif count_loans(m) >= 2:
            print(f"{m} at limit")
        elif b in loans:
            print(f"{b} on loan until day {loans[b][1]}")
        elif b in held and held[b] != m:
            print(f"{b} held for {held[b]}")
        else:
            held.pop(b, None)
            loans[b] = (m, day + 14)
            print(f"{m} borrowed {b}, due day {day + 14}")
    elif kind == "return":
        b = arg
        if b not in loans or loans[b][0] != m:
            print(f"{m} does not have {b}")
            continue
        due = loans.pop(b)[1]
        if day > due:
            late = day - due
            fee = min(25 * late, 500)
            balance[m] += fee
            print(f"{m} returned {b}, {late} days late, fee {money(fee)}")
        else:
            print(f"{m} returned {b}")
        if waiting[b]:
            held[b] = waiting[b].pop(0)
            print(f"{b} held for {held[b]}")
    elif kind == "hold":
        b = arg
        if b in loans and loans[b][0] == m:
            print(f"{m} already has {b}")
        elif b not in loans and b not in held:
            print(f"{b} is available")
        elif m in waiting[b] or held.get(b) == m:
            print(f"{m} already waiting for {b}")
        else:
            waiting[b].append(m)
            print(f"{m} waiting for {b}, position {len(waiting[b])}")
    else:
        x = int(arg)
        if x > balance[m]:
            print(f"{m} paid {money(balance[m])}, change {money(x - balance[m])}")
            balance[m] = 0
        else:
            balance[m] -= x
            print(f"{m} paid {money(x)}, owes {money(balance[m])}")

for m in MEMBERS:
    books = [b for b in BOOKS if b in loans and loans[b][0] == m]
    print(f"{m} owes {money(balance[m])}, has {', '.join(books) or 'none'}")
