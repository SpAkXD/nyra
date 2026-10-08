operations = ("add 1/2; sub 3/4; mul -2/3; div 5/6; add 7/3; div 0/5; mul 4/-6; add 10/4; sub -1/12; mul 0/7; "
              "add -9/4; div -3/2; sub 5/3; add 2/3; div 1/-4")


def gcd(a, b):
    while b:
        a, b = b, a % b
    return a


def reduce(n, d):
    if d < 0:
        n, d = -n, -d
    g = gcd(abs(n), d)
    return n // g, d // g


def show(n, d):
    if d == 1:
        return str(n)
    sign = "-" if n < 0 else ""
    whole, rest = abs(n) // d, abs(n) % d
    if whole == 0:
        return f"{sign}{rest}/{d}"
    return f"{sign}{whole} {rest}/{d}"


total = (0, 1)
best = None  # (n, d, operation number)
for k, op in enumerate(operations.split("; "), 1):
    word, frac = op.split(" ")
    a, b = (int(x) for x in frac.split("/"))
    n, d = total
    if word == "add":
        new = reduce(n * b + a * d, d * b)
    elif word == "sub":
        new = reduce(n * b - a * d, d * b)
    elif word == "mul":
        new = reduce(n * a, d * b)
    elif a == 0:
        print(f"{op}: cannot divide by zero, total {show(*total)}")
        continue
    else:
        new = reduce(n * b, d * a)
    total = new
    print(f"{op}: total {show(*total)}")
    if best is None or total[0] * best[1] > best[0] * total[1]:
        best = (total[0], total[1], k)
print(f"largest total {show(best[0], best[1])} after operation {best[2]}")
