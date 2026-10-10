items = [("apple", 125, 3), ("bread", 399, 1), ("milk", 89, 12), ("cheese", 1450, 2)]

def fmt(cents):
    return "%d.%02d" % (cents // 100, cents % 100)

lines = []
subtotal = 0
for name, price, qty in items:
    t = price * qty
    subtotal += t
    lines.append((name, t))

tax = (subtotal * 825 + 5000) // 10000
total = subtotal + tax
lines.append(("subtotal", subtotal))
lines.append(("tax", tax))
lines.append(("total", total))

for name, amt in lines:
    print(("%-8s%8s" % (name, fmt(amt))).rstrip())
