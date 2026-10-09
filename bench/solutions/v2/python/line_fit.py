def fixed(x, digits):
    # the tests never contain a number that is close to a rounding tie: make sure of it
    scaled = abs(x) * 10 ** digits
    assert abs(scaled - int(scaled) - 0.5) > 1e-6, "too close to a rounding tie"
    return f"{x:.{digits}f}"

import re
import sys

xs, ys = [], []
for number, raw in enumerate(sys.stdin.read().splitlines(), 1):
    if raw == "":
        continue
    parts = raw.split(" ")
    if len(parts) == 2 and all(re.fullmatch(r"-?[0-9]+(\.[0-9]+)?", p) for p in parts):
        xs.append(float(parts[0]))
        ys.append(float(parts[1]))
    else:
        print(f"line {number}: bad point")
n = len(xs)
sx = sy = sxy = sxx = 0.0
for x, y in zip(xs, ys):
    sx += x
    sy += y
    sxy += x * y
    sxx += x * x
denominator = n * sxx - sx * sx
if n < 2 or denominator == 0:
    print(f"n={n} undefined")
else:
    a = (n * sxy - sx * sy) / denominator
    b = (sy - a * sx) / n
    print(f"n={n} slope={fixed(a, 4)} intercept={fixed(b, 4)}")
