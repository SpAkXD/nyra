import re
import sys

PAIRS = [(1000, "M"), (900, "CM"), (500, "D"), (400, "CD"), (100, "C"), (90, "XC"), (50, "L"), (40, "XL"),
         (10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I")]
LETTER = {"I": 1, "V": 5, "X": 10, "L": 50, "C": 100, "D": 500, "M": 1000}


def to_roman(n):
    out = ""
    for value, symbol in PAIRS:
        while n >= value:
            out += symbol
            n -= value
    return out


def from_roman(s):
    total = 0
    for i, c in enumerate(s):
        v = LETTER[c]
        if i + 1 < len(s) and v < LETTER[s[i + 1]]:
            total -= v
        else:
            total += v
    return total


for raw in sys.stdin.read().splitlines():
    s = raw.strip(" ")
    if s == "":
        continue
    if re.fullmatch(r"[0-9]+", s):
        n = int(s)
        print(to_roman(n) if 1 <= n <= 3999 else "INVALID")
    elif re.fullmatch(r"[IVXLCDM]+", s):
        n = from_roman(s)
        print(n if 1 <= n <= 3999 and to_roman(n) == s else "INVALID")
    else:
        print("INVALID")
