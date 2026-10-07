limit = 60
is_prime = [True] * limit
is_prime[0] = is_prime[1] = False
for i in range(2, limit):
    if is_prime[i]:
        for multiple in range(i * i, limit, i):
            is_prime[multiple] = False
for i in range(limit):
    if is_prime[i]:
        print(i)
