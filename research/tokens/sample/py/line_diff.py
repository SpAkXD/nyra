def diff(a, b):
    n, m = len(a), len(b)
    L = [[0] * (m + 1) for _ in range(n + 1)]
    for i in range(n - 1, -1, -1):
        for j in range(m - 1, -1, -1):
            if a[i] == b[j]:
                L[i][j] = L[i + 1][j + 1] + 1
            else:
                L[i][j] = max(L[i + 1][j], L[i][j + 1])

    script = []
    i = j = 0
    while i < n or j < m:
        if i < n and j < m and a[i] == b[j]:
            script.append(('=', a[i]))
            i += 1
            j += 1
        elif i < n and (j == m or L[i + 1][j] >= L[i][j + 1]):
            script.append(('-', a[i]))
            i += 1
        else:
            script.append(('+', b[j]))
            j += 1

    out = []
    k = 0
    while k < len(script):
        if script[k][0] == '=':
            e = k
            while e < len(script) and script[e][0] == '=':
                e += 1
            run = e - k
            if run >= 3:
                out.append("= (%d unchanged)" % run)
            else:
                for t in range(k, e):
                    out.append("= " + script[t][1])
            k = e
        else:
            out.append(script[k][0] + " " + script[k][1])
            k += 1

    I = sum(1 for s in script if s[0] == '+')
    D = sum(1 for s in script if s[0] == '-')
    U = sum(1 for s in script if s[0] == '=')
    out.append("summary: +%d -%d =%d" % (I, D, U))
    return out

a1 = ["alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa"]
b1 = ["alpha", "gamma", "delta", "beta", "epsilon", "zeta", "eta", "lambda", "theta", "kappa", "mu"]
a2 = ["x = 1", "y = 2", "print(x)", "print(y)", "z = x + y", "print(z)"]
b2 = ["y = 2", "x = 1", "print(x)", "z = x + y", "print(z)", "print(y)"]

print("diff 1")
for line in diff(a1, b1):
    print(line)
print("diff 2")
for line in diff(a2, b2):
    print(line)
