import bisect
import sys

for line in sys.stdin.read().splitlines():
    values = [int(t) for t in line.split(" ") if t != ""]
    strict, loose = [], []
    for v in values:
        i = bisect.bisect_left(strict, v)
        if i == len(strict):
            strict.append(v)
        else:
            strict[i] = v
        j = bisect.bisect_right(loose, v)
        if j == len(loose):
            loose.append(v)
        else:
            loose[j] = v
    print(f"strict={len(strict)} nondecreasing={len(loose)}")
