def gcd(a, b):
    while b:
        a, b = b, a % b
    return a

pairs = [(48, 18), (1071, 462), (17, 5), (1000000, 250000), (270, 192)]
for a, b in pairs:
    print(gcd(a, b))
