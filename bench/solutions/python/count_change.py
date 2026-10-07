amount = 175
coins = [1, 5, 10, 25, 50]

# ways[a] = the number of ways to make a cents with the coins considered so far
ways = [1] + [0] * amount
for coin in coins:
    for a in range(coin, amount + 1):
        ways[a] += ways[a - coin]
print(ways[amount])
