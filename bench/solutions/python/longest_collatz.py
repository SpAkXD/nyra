def steps(n):
    count = 0
    while n != 1:
        n = n // 2 if n % 2 == 0 else 3 * n + 1
        count += 1
    return count


best, best_steps = 1, 0
for start in range(1, 10000):
    s = steps(start)
    if s > best_steps:
        best, best_steps = start, s
print(best)
print(best_steps)
