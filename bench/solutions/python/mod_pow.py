result = 1
for _ in range(200):
    result = result * 3 % 1000007
print(result)
