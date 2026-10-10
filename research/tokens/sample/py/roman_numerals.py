def to_roman(n):
    vals = [(1000, "M"), (900, "CM"), (500, "D"), (400, "CD"),
            (100, "C"), (90, "XC"), (50, "L"), (40, "XL"),
            (10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I")]
    s = ""
    for v, r in vals:
        while n >= v:
            s += r
            n -= v
    return s

for n in [4, 9, 14, 40, 90, 400, 1994, 2024]:
    print(to_roman(n))
