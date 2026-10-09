from fractions import Fraction

ops = "add 1/2; sub 3/4; mul -2/3; div 5/6; add 7/3; div 0/5; mul 4/-6; add 10/4; sub -1/12; mul 0/7; add -9/4; div -3/2; sub 5/3; add 2/3; div 1/-4"
ops = [o.strip() for o in ops.split(";")]

def fmt(x):
    if x.denominator == 1:
        return str(x.numerator)
    if abs(x) < 1:
        return f"{x.numerator}/{x.denominator}"
    sign = "-" if x < 0 else ""
    a = abs(x)
    w = a.numerator // a.denominator
    r = a.numerator % a.denominator
    return f"{sign}{w} {r}/{a.denominator}"

total = Fraction(0)
best = None
bestk = None
for i, op in enumerate(ops, 1):
    word, fr = op.split()
    a, b = fr.split("/")
    a, b = int(a), int(b)
    f = Fraction(a, b)
    if word == "div" and f == 0:
        print(f"{op}: cannot divide by zero, total {fmt(total)}")
        continue
    if word == "add":
        total += f
    elif word == "sub":
        total -= f
    elif word == "mul":
        total *= f
    elif word == "div":
        total /= f
    print(f"{op}: total {fmt(total)}")
    if best is None or total > best:
        best = total
        bestk = i
print(f"largest total {fmt(best)} after operation {bestk}")
