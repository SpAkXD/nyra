items = [("apple", 125, 3), ("bread", 399, 1), ("milk", 89, 12), ("cheese", 1450, 2)]


def money(cents):
    return f"{cents // 100}.{cents % 100:02d}"


def line(label, cents):
    print(f"{label:<8}{money(cents):>8}")


subtotal = 0
for name, price, quantity in items:
    total = price * quantity
    subtotal += total
    line(name, total)
tax = (subtotal * 825 + 5000) // 10000
line("subtotal", subtotal)
line("tax", tax)
line("total", subtotal + tax)
