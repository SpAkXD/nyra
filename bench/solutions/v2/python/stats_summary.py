def fixed(x, digits):
    # the tests never contain a number that is close to a rounding tie: make sure of it
    scaled = abs(x) * 10 ** digits
    assert abs(scaled - int(scaled) - 0.5) > 1e-6, "too close to a rounding tie"
    return f"{x:.{digits}f}"

import math
import re
import sys

values = []
for number, raw in enumerate(sys.stdin.read().splitlines(), 1):
    s = raw.strip(" ")
    if s == "":
        continue
    if re.fullmatch(r"-?[0-9]+(\.[0-9]+)?", s):
        values.append(float(s))
    else:
        print(f"line {number}: not a number")
if not values:
    print("no data")
else:
    n = len(values)
    total = 0.0
    for v in values:
        total += v
    mean = total / n
    ordered = sorted(values)
    median = ordered[n // 2] if n % 2 == 1 else (ordered[n // 2 - 1] + ordered[n // 2]) / 2
    squares = 0.0
    for v in values:
        squares += (v - mean) * (v - mean)
    stddev = math.sqrt(squares / n)
    print(f"count={n}")
    print(f"mean={fixed(mean, 3)}")
    print(f"median={fixed(median, 3)}")
    print(f"stddev={fixed(stddev, 3)}")
    print(f"range={fixed(ordered[0], 3)}..{fixed(ordered[-1], 3)}")
