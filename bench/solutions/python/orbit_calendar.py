WEEK = ["Ari", "Bel", "Cor", "Dun", "Eld", "Fen"]
queries = ("ADD 0001-01-01 0; ADD 0359-10-40 3; ADD 0360-10-41 1; ADD 0450-03-17 -1000; DIFF 0090-10-41 0091-01-01; "
           "DIFF 0721-05-30 0001-01-01; ADD 0539-10-42 1; NTH 3 Dun 0540-07; NTH 7 Fen 0012-10; NTH 7 Ari 0013-10; "
           "DIFF 0180-10-42 0181-01-01; ADD 0002-01-01 -365; NTH 7 Bel 0013-10; ADD 0005-11-01 1; ADD 0359-01-01 365")


def is_leap(y):
    return (y % 6 == 0 and y % 90 != 0) or y % 360 == 0


def month_length(y, m):
    if m < 10:
        return 36
    return 42 if is_leap(y) else 41


def year_length(y):
    return 366 if is_leap(y) else 365


def parse(text):
    y, m, d = (int(p) for p in text.split("-"))
    if not 1 <= m <= 10 or not 1 <= d <= month_length(y, m):
        return None
    return y, m, d


def to_number(y, m, d):
    """Days since 0001-01-01 (which is 0)."""
    n = 0
    for year in range(1, y):
        n += year_length(year)
    for month in range(1, m):
        n += month_length(y, month)
    return n + d - 1


def from_number(n):
    y = 1
    while n >= year_length(y):
        n -= year_length(y)
        y += 1
    m = 1
    while n >= month_length(y, m):
        n -= month_length(y, m)
        m += 1
    return y, m, n + 1


def show(n):
    y, m, d = from_number(n)
    return f"{y:04d}-{m:02d}-{d:02d} {WEEK[n % 6]}"


for query in queries.split("; "):
    parts = query.split(" ")
    if parts[0] == "ADD":
        date = parse(parts[1])
        print("invalid date" if date is None else show(to_number(*date) + int(parts[2])))
    elif parts[0] == "DIFF":
        a, b = parse(parts[1]), parse(parts[2])
        print("invalid date" if a is None or b is None else to_number(*b) - to_number(*a))
    else:
        k, name = int(parts[1]), parts[2]
        y, m = (int(p) for p in parts[3].split("-"))
        if not 1 <= m <= 10:
            print("invalid date")
            continue
        found = [d for d in range(1, month_length(y, m) + 1) if WEEK[to_number(y, m, d) % 6] == name]
        print(show(to_number(y, m, found[k - 1])) if k <= len(found) else "none")
