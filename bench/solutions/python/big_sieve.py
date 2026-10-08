LIMIT = 2_000_000
is_prime = [True] * LIMIT
is_prime[0] = is_prime[1] = False
i = 2
while i * i < LIMIT:
    if is_prime[i]:
        for j in range(i * i, LIMIT, i):
            is_prime[j] = False
    i += 1
count = 0
total = 0
largest = 0
twins = 0
for n in range(2, LIMIT):
    if is_prime[n]:
        count += 1
        total += n
        largest = n
        if n + 2 < LIMIT and is_prime[n + 2]:
            twins += 1
print(count)
print(total)
print(largest)
print(twins)
