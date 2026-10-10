N = 500_000
M = 1_000_000_007
xs = []
x = 12345
for _ in range(N):
    x = x * 48271 % 2147483647
    xs.append(x)
xs.sort()
check = 0
for i in range(N):
    check = (check + (i + 1) * xs[i]) % M
print(xs[0])
print(xs[N - 1])
print(xs[N // 2])
print(check)
