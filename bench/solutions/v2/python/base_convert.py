import re
import sys

DIGITS = "0123456789abcdefghijklmnopqrstuvwxyz"


def base_ok(token):
    return re.fullmatch(r"[0-9]{1,2}", token) is not None and 2 <= int(token) <= 36


def to_base(n, base):
    if n == 0:
        return "0"
    out = ""
    m = abs(n)
    while m > 0:
        out = DIGITS[m % base] + out
        m //= base
    return ("-" if n < 0 else "") + out


for line in sys.stdin.read().splitlines():
    parts = line.split(" ")
    if len(parts) != 3 or not base_ok(parts[1]) or not base_ok(parts[2]):
        print("invalid")
        continue
    number, src, dst = parts[0], int(parts[1]), int(parts[2])
    body = number[1:] if number.startswith("-") else number
    value = 0
    ok = body != ""
    for c in body.lower():
        d = DIGITS.find(c)
        if d < 0 or d >= src:
            ok = False
            break
        value = value * src + d
    if not ok:
        print("invalid")
    else:
        print(to_base(-value if number.startswith("-") else value, dst))
