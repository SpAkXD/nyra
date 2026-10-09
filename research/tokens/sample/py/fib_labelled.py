a, b = 0, 1
for n in range(10):
    print(f"fib({n}) = {a}")
    a, b = b, a + b
