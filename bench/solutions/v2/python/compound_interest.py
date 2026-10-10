def fixed(x, digits):
    # the tests never contain a number that is close to a rounding tie: make sure of it
    scaled = abs(x) * 10 ** digits
    assert abs(scaled - int(scaled) - 0.5) > 1e-6, "too close to a rounding tie"
    return f"{x:.{digits}f}"

import sys

principal, rate, years, periods = (float(t) for t in sys.stdin.read().split())
balance = principal
for year in range(1, int(years) + 1):
    for _ in range(int(periods)):
        balance = balance * (1 + rate / 100 / periods)
    print(f"Year {year}: {fixed(balance, 2)}")
print(f"interest earned: {fixed(balance - principal, 2)}")
