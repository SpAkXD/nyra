months = [
    "deposit 1200.00; withdraw 1500.00; withdraw 197.50",
    "deposit 4000.50; withdraw 20.00",
    "withdraw 3978.75; deposit 6000.00",
    "withdraw 7500.00; withdraw 1997.76",
    "withdraw 5011.26",
    "withdraw 2.00; deposit 0.50",
]


def cents(text):
    whole, frac = text.split(".")
    return int(whole) * 100 + int(frac)


def money(c):
    return f"{c // 100}.{c % 100:02d}"


def monthly_interest(minimum):
    p1 = min(minimum, 100000)
    p2 = min(max(minimum - 100000, 0), 400000)
    p3 = max(minimum - 500000, 0)
    yearly_times_100 = p1 * 150 + p2 * 240 + p3 * 300  # cents * 10000 (percent * 100)
    q, r = divmod(yearly_times_100, 120000)
    if r * 2 > 120000 or (r * 2 == 120000 and q % 2 == 1):
        q += 1
    return q


balance = 0
total_interest = 0
total_fees = 0
for number, line in enumerate(months, 1):
    lowest = balance
    for transaction in line.split("; "):
        kind, amount = transaction.split(" ")
        x = cents(amount)
        if kind == "deposit":
            balance += x
        elif x > balance:
            print(f"month {number}: withdraw {amount} rejected")
        else:
            balance -= x
        lowest = min(lowest, balance)
    interest = monthly_interest(lowest)
    balance += interest
    fee = min(300, balance) if lowest < 50000 else 0
    balance -= fee
    total_interest += interest
    total_fees += fee
    print(f"month {number}: min {money(lowest)} interest {money(interest)} fee {money(fee)} balance {money(balance)}")
print(f"total interest {money(total_interest)} fees {money(total_fees)}")
