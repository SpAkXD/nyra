def main():
    n = 120
    A = [[(31*i + 17*j + 7) % 100 for j in range(n)] for i in range(n)]
    B = [[(13*i + 29*j + 3) % 100 for j in range(n)] for i in range(n)]
    Bt = list(zip(*B))
    C = [[sum(a*b for a, b in zip(row, col)) for col in Bt] for row in A]
    tr = sum(C[i][i] for i in range(n))
    tot = sum(sum(r) for r in C)
    print(tr)
    print(tot)
    print(C[0][119])
    print(C[119][0])

main()
