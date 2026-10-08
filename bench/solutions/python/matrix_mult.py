N = 120
a = [[(i * 31 + j * 17 + 7) % 100 for j in range(N)] for i in range(N)]
b = [[(i * 13 + j * 29 + 3) % 100 for j in range(N)] for i in range(N)]
c = [[0] * N for _ in range(N)]
for i in range(N):
    for j in range(N):
        s = 0
        for k in range(N):
            s += a[i][k] * b[k][j]
        c[i][j] = s
trace = 0
total = 0
for i in range(N):
    trace += c[i][i]
    for j in range(N):
        total += c[i][j]
print(trace)
print(total)
print(c[0][N - 1])
print(c[N - 1][0])
