# quicksort and the built-in sort of 1,000,000 ints (reference for sort.nyra)
import sys

sys.setrecursionlimit(10000)


def quicksort(xs, lo, hi):
    if lo >= hi:
        return
    pivot = xs[(lo + hi) // 2]
    i, j = lo, hi
    while i <= j:
        while xs[i] < pivot:
            i += 1
        while xs[j] > pivot:
            j -= 1
        if i <= j:
            xs[i], xs[j] = xs[j], xs[i]
            i += 1
            j -= 1
    quicksort(xs, lo, j)
    quicksort(xs, i, hi)


seed = 7
xs = []
for _ in range(1000000):
    seed = (seed * 1103515245 + 12345) % 2147483648
    xs.append((seed // 64) % 1000000)
ys = list(xs)
quicksort(xs, 0, len(xs) - 1)
ys.sort()
print("true" if xs == ys else "false")
check = 0
for x in xs:
    check = (check * 31 + x) % 1000000007
print(check)
