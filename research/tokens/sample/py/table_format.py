rows = [
    ("bolts", 12000, 5, "bulk"),
    ("hex nuts", 350, 12, "galvanized"),
    ("washers", 7, 123456, "special order"),
    ("anchor", 1, 99, ""),
    ("rivets", 1500, 3, "aluminium"),
    ("brackets", 24, 1050, "left-hand only"),
]

def money(c):
    return "{:,}.{:02d}".format(c // 100, c % 100)

def note(n):
    return n if len(n) <= 10 else n[:9] + "~"

header = ["item", "qty", "price", "amount", "note"]
aligns = ["l", "r", "r", "r", "c"]
data = []
tq = 0
ta = 0
for it, q, p, n in rows:
    a = q * p
    tq += q
    ta += a
    data.append([it, "{:,}".format(q), money(p), money(a), note(n)])
total = ["TOTAL", "{:,}".format(tq), "", money(ta), ""]

allrows = [header] + data + [total]
widths = [max(len(r[i]) for r in allrows) for i in range(5)]

def fmt(cell, w, al):
    pad = w - len(cell)
    if al == "l":
        return cell + " " * pad
    if al == "r":
        return " " * pad + cell
    l = pad // 2
    return " " * l + cell + " " * (pad - l)

def line(r):
    return "| " + " | ".join(fmt(c, widths[i], aligns[i]) for i, c in enumerate(r)) + " |"

def border(ch):
    return "+" + "".join(ch * (w + 2) + "+" for w in widths)

out = [border("-"), line(header), border("=")]
for r in data:
    out.append(line(r))
out += [border("-"), line(total), border("-")]
print("\n".join(out))
