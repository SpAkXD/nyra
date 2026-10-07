def gcd(a, b):
    while b != 0:
        a, b = b, a % b
    return a


for a, b in ((48, 18), (1071, 462), (17, 5), (1000000, 250000), (270, 192)):
    print(gcd(a, b))
