def to_binary(n):
    digits = ""
    while n > 0:
        digits = str(n % 2) + digits
        n //= 2
    return digits


for n in (5, 10, 255, 1024):
    print(to_binary(n))
