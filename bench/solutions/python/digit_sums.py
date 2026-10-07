def digit_sum(n):
    total = 0
    while n > 0:
        total += n % 10
        n //= 10
    return total


for n in (12345, 9999, 100000, 987654321):
    print(digit_sum(n))
