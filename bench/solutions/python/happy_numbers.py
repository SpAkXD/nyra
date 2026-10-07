def next_value(n):
    return sum(int(digit) ** 2 for digit in str(n))


def is_happy(n):
    seen = set()
    while n != 1 and n not in seen:
        seen.add(n)
        n = next_value(n)
    return n == 1


found = 0
n = 1
while found < 10:
    if is_happy(n):
        print(n)
        found += 1
    n += 1
