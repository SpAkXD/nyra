pairs = [
    ("alpha beta gamma delta epsilon zeta eta theta iota kappa".split(" "),
     "alpha gamma delta beta epsilon zeta eta lambda theta kappa mu".split(" ")),
    (["x = 1", "y = 2", "print(x)", "print(y)", "z = x + y", "print(z)"],
     ["y = 2", "x = 1", "print(x)", "z = x + y", "print(z)", "print(y)"]),
]


def diff(a, b):
    n, m = len(a), len(b)
    lcs = [[0] * (m + 1) for _ in range(n + 1)]
    for i in range(n - 1, -1, -1):
        for j in range(m - 1, -1, -1):
            if a[i] == b[j]:
                lcs[i][j] = lcs[i + 1][j + 1] + 1
            else:
                lcs[i][j] = max(lcs[i + 1][j], lcs[i][j + 1])
    script = []
    i = j = 0
    while i < n or j < m:
        if i < n and j < m and a[i] == b[j]:
            script.append(("=", a[i]))
            i += 1
            j += 1
        elif i < n and (j == m or lcs[i + 1][j] >= lcs[i][j + 1]):
            script.append(("-", a[i]))
            i += 1
        else:
            script.append(("+", b[j]))
            j += 1
    return script


for number, (old, new) in enumerate(pairs, 1):
    print(f"diff {number}")
    script = diff(old, new)
    k = 0
    while k < len(script):
        kind, line = script[k]
        if kind == "=":
            end = k
            while end < len(script) and script[end][0] == "=":
                end += 1
            if end - k >= 3:
                print(f"= ({end - k} unchanged)")
            else:
                for _, same in script[k:end]:
                    print(f"= {same}")
            k = end
        else:
            print(f"{kind} {line}")
            k += 1
    counts = {kind: sum(1 for s, _ in script if s == kind) for kind in "+-="}
    print(f"summary: +{counts['+']} -{counts['-']} ={counts['=']}")
