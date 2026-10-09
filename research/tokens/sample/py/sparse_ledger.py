from collections import defaultdict

cells = defaultdict(int)
cmds = """add rent 1 900; add food 1 250; add food 2 -40; move rent 1 food 2 100; move food 2 rent 3 200; add fun 12 75; clear fun 12; clear fun 12; add rent 3 1300; move rent 3 rent 3 50; add zoo 5 0; add tax 4 -1300; move rent 3 tax 4 1300; add food 1 -250; add car 7 15; move car 7 car 8 15; add tax 10 -75; add food 10 12000; move food 10 rent 10 2500"""

for c in cmds.split(";"):
    p = c.split()
    if not p:
        continue
    if p[0] == "add":
        k = (p[1], int(p[2]))
        cells[k] += int(p[3])
        print(f"{p[1]} {p[2]} = {cells[k]}")
    elif p[0] == "move":
        a = (p[1], int(p[2]))
        b = (p[3], int(p[4]))
        n = int(p[5])
        if cells[a] < n:
            print("move rejected")
        else:
            cells[a] -= n
            cells[b] += n
            print(f"{a[0]} {a[1]} = {cells[a]}, {b[0]} {b[1]} = {cells[b]}")
    elif p[0] == "clear":
        k = (p[1], int(p[2]))
        if cells[k] == 0:
            print(f"{p[1]} {p[2]} already empty")
        else:
            print(f"{p[1]} {p[2]} cleared (was {cells[k]})")
            cells[k] = 0

nz = {k: v for k, v in cells.items() if v != 0}
accts = sorted({k[0] for k in nz})
months = sorted({k[1] for k in nz})

rows = [["account"] + [str(m) for m in months] + ["total"]]
for a in accts:
    r = [a]
    for m in months:
        v = nz.get((a, m), 0)
        r.append("-" if v == 0 else str(v))
    r.append(str(sum(v for k, v in nz.items() if k[0] == a)))
    rows.append(r)

tr = ["total"]
for m in months:
    tr.append(str(sum(v for k, v in nz.items() if k[1] == m)))
tr.append(str(sum(nz.values())))
rows.append(tr)

ncol = len(rows[0])
w = [max(len(r[i]) for r in rows) for i in range(ncol)]
for r in rows:
    parts = [r[0].ljust(w[0])] + [r[i].rjust(w[i]) for i in range(1, ncol)]
    print("  ".join(parts).rstrip())
