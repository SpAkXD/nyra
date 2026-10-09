import re
import sys


def parse_money(text):
    m = re.fullmatch(r"([0-9]+)(?:\.([0-9]{1,2}))?", text)
    if not m:
        return None
    return int(m.group(1)) * 100 + int((m.group(2) or "0").ljust(2, "0"))


def money(cents):
    return f"{cents // 100}.{cents % 100:02d}"


items = []
rate_basis_points = 0
for number, line in enumerate(sys.stdin.read().splitlines(), 1):
    if line.strip(" ") == "":
        continue
    if line.startswith("tax="):
        rate = parse_money(line[4:])
        if rate is None:
            print(f"line {number}: invalid")
        else:
            rate_basis_points = rate
        continue
    parts = line.split(";")
    if len(parts) == 3 and parts[0] != "" and re.fullmatch(r"[0-9]{1,4}", parts[1]) and 1 <= int(parts[1]) <= 9999:
        price = parse_money(parts[2])
        if price is not None:
            qty = int(parts[1])
            items.append(qty * price)
            print(f"{parts[0]}: {qty} x {money(price)} = {money(qty * price)}")
            continue
    print(f"line {number}: invalid")
subtotal = sum(items)
tax = (subtotal * rate_basis_points + 5000) // 10000
print(f"subtotal={money(subtotal)}")
print(f"tax={money(tax)}")
print(f"total={money(subtotal + tax)}")
