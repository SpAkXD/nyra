def reverse(n):
    result = 0
    while n > 0:
        result = result * 10 + n % 10
        n //= 10
    return result


for n in (12345, 1200, 907, 86420):
    print(reverse(n))
