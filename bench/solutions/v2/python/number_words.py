import re
import sys

ONES = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve",
        "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen"]
TENS = ["", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"]


def below_thousand(n):
    words = []
    if n >= 100:
        words += [ONES[n // 100], "hundred"]
        n %= 100
    if n >= 20:
        words.append(TENS[n // 10] + ("-" + ONES[n % 10] if n % 10 else ""))
    elif n > 0:
        words.append(ONES[n])
    return words


def spell(n):
    if n == 0:
        return "zero"
    words = []
    for size, name in ((1_000_000, "million"), (1000, "thousand"), (1, "")):
        part = n // size
        n %= size
        if part:
            words += below_thousand(part) + ([name] if name else [])
    return " ".join(words)


for raw in sys.stdin.read().splitlines():
    s = raw.strip(" ")
    if s == "":
        continue
    if not re.fullmatch(r"-?[0-9]+", s) or abs(int(s)) > 999_999_999:
        print("invalid")
        continue
    n = int(s)
    print(("minus " if n < 0 else "") + spell(abs(n)))
