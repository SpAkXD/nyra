rows = "ABCDEF"
order = "CDBEAF"
seats = {r: [None]*11 for r in rows}
broken = {("C",5),("C",6),("E",1)}
for r,s in broken:
    seats[r][s] = "BROKEN"

reqs = "ann 4; bob 4; cat 2; dan 10; eve 1; fay 6; cancel bob; gus 5; hal 3; cancel zed; ivy 9; jo 2; kai 8; lea 3"
out = []
for req in reqs.split(";"):
    parts = req.split()
    if parts[0] == "cancel":
        name = parts[1]
        n = 0
        for r in rows:
            for s in range(1, 11):
                if seats[r][s] == name:
                    seats[r][s] = None
                    n += 1
        if n:
            out.append(f"{name} cancelled ({n} seats)")
        else:
            out.append(f"{name} has no booking")
    else:
        name, k = parts[0], int(parts[1])
        done = False
        for r in order:
            best = None
            for start in range(1, 12 - k):
                if all(seats[r][s] is None for s in range(start, start + k)):
                    mid = start + (k - 1) / 2
                    d = abs(mid - 5.5)
                    if best is None or d < best[0]:
                        best = (d, start)
            if best:
                st = best[1]
                for s in range(st, st + k):
                    seats[r][s] = name
                if k == 1:
                    out.append(f"{name}: {r}{st}")
                else:
                    out.append(f"{name}: {r}{st}-{r}{st+k-1}")
                done = True
                break
        if not done:
            out.append(f"{name}: no room")

for r in rows:
    line = ""
    for s in range(1, 11):
        v = seats[r][s]
        line += "x" if v == "BROKEN" else ("." if v is None else "#")
    out.append(f"{r} {line}")
print("\n".join(out))
