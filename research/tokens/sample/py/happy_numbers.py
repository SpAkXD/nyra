def is_happy(n):
    seen = set()
    while n != 1 and n not in seen:
        seen.add(n)
        n = sum(int(d) ** 2 for d in str(n))
    return n == 1

count = 0
n = 1
while count < 10:
    if is_happy(n):
        print(n)
        count += 1
    n += 1
