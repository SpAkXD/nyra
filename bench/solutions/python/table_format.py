rows = [("bolts", 12000, 5, "bulk"), ("hex nuts", 350, 12, "galvanized"), ("washers", 7, 123456, "special order"),
        ("anchor", 1, 99, ""), ("rivets", 1500, 3, "aluminium"), ("brackets", 24, 1050, "left-hand only")]


def grouped(n):
    text = str(n)
    parts = []
    while len(text) > 3:
        parts.insert(0, text[-3:])
        text = text[:-3]
    parts.insert(0, text)
    return ",".join(parts)


def dollars(cents):
    return f"{grouped(cents // 100)}.{cents % 100:02d}"


def cut(note):
    return note if len(note) <= 10 else note[:9] + "~"


header = ["item", "qty", "price", "amount", "note"]
align = ["left", "right", "right", "right", "center"]
body = [[item, grouped(qty), dollars(price), dollars(qty * price), cut(note)] for item, qty, price, note in rows]
total = ["TOTAL", grouped(sum(r[1] for r in rows)), "", dollars(sum(r[1] * r[2] for r in rows)), ""]
widths = [max(len(r[i]) for r in [header] + body + [total]) for i in range(len(header))]


def fit(text, width, how):
    extra = width - len(text)
    if how == "left":
        return text + " " * extra
    if how == "right":
        return " " * extra + text
    return " " * (extra // 2) + text + " " * (extra - extra // 2)


def border(ch):
    return "+" + "+".join(ch * (w + 2) for w in widths) + "+"


def row(cells):
    return "| " + " | ".join(fit(c, w, a) for c, w, a in zip(cells, widths, align)) + " |"


print(border("-"))
print(row(header))
print(border("="))
for cells in body:
    print(row(cells))
print(border("-"))
print(row(total))
print(border("-"))
