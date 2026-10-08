A = [[2, -1, 0], [1, 3, -2], [0, 4, 1]]
B = [[1, 2], [-3, 0], [5, -1]]
C = [[2, -1], [1, 3]]
D = [[3, 1, 2, -1], [1, 2, 0, -2], [4, -1, 6, -3], [5, 0, 2, 1]]


def multiply(x, y):
    return [[sum(x[i][k] * y[k][j] for k in range(len(y))) for j in range(len(y[0]))] for i in range(len(x))]


def transpose(x):
    return [[x[i][j] for i in range(len(x))] for j in range(len(x[0]))]


def det(m):
    if len(m) == 1:
        return m[0][0]
    total = 0
    for j in range(len(m)):
        minor = [row[:j] + row[j + 1:] for row in m[1:]]
        sign = 1 if j % 2 == 0 else -1
        total += sign * m[0][j] * det(minor)
    return total


def show(title, m):
    print(title)
    widths = [max(len(str(m[i][j])) for i in range(len(m))) for j in range(len(m[0]))]
    for row in m:
        print("  ".join(str(v).rjust(w) for v, w in zip(row, widths)))


ab = multiply(A, B)
show("A*B", ab)
show("(A*B)^T", transpose(ab))
power = C
for _ in range(5):
    power = multiply(power, C)
show("C^6", power)
aa = multiply(A, A)
result = [[aa[i][j] - 3 * A[i][j] + (2 if i == j else 0) for j in range(3)] for i in range(3)]
show("A*A-3A+2I", result)
print(f"det(D) = {det(D)}")
