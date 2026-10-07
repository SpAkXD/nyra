def fib(n):
    if n <= 2:
        return 1
    return fib(n - 1) + fib(n - 2)


for n in (10, 20, 25):
    print(fib(n))
