def gcd(a, b):
    while b != 0:
        a, b = b, a % b
    return a


result = 1
for n in range(2, 21):
    result = result // gcd(result, n) * n
print(result)
