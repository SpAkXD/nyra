# recursion: fib(n) for n = 0 to 35, summed (reference for fib.nyra)
def fib(n):
    return n if n < 2 else fib(n - 1) + fib(n - 2)


total = 0
for n in range(36):
    total += fib(n)
print(total)
