import sys
from fractions import Fraction


def fixed(x, digits):
    # the tests never contain a rounding tie (exactly halfway between two outputs): make sure of it
    scaled = Fraction(x) * 10 ** digits * 2
    assert not (scaled.denominator == 1 and scaled.numerator % 2 == 1), "rounding tie"
    return f"{x:.{digits}f}"


customers = []
for line in sys.stdin.read().splitlines():
    parts = [t for t in line.split(" ") if t != ""]
    if len(parts) == 2:
        customers.append((int(parts[0]), int(parts[1])))
print(f"customers={len(customers)}")
if customers:
    free_at = 0
    waits = []
    busy = 0
    for arrival, service in customers:
        start = max(arrival, free_at)
        waits.append(start - arrival)
        free_at = start + service
        busy += service
    span = free_at - customers[0][0]
    print(f"average wait={fixed(sum(waits) / len(waits), 2)} max wait={max(waits)}")
    print(f"busy={fixed(busy * 100 / span, 1)}%")
