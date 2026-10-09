results_str = "Ash 2-1 Bay; Cove 2-0 Ash; Dale 2-2 Ash; Ash 0-2 Elm; Fir 0-0 Ash; Bay 3-2 Cove; Dale 0-2 Bay; Elm 1-2 Bay; Bay 2-2 Fir; Cove 1-3 Dale; Cove 2-0 Elm; Fir 1-0 Cove; Elm 0-0 Dale; Dale 2-2 Fir; Elm 3-2 Fir"

matches = []
for part in results_str.split(";"):
    part = part.strip()
    if not part:
        continue
    toks = part.split()
    home, score, away = toks[0], toks[1], toks[2]
    x, y = map(int, score.split("-"))
    matches.append((home, x, y, away))

stats = {}
def get(t):
    if t not in stats:
        stats[t] = {"P": 0, "W": 0, "D": 0, "L": 0, "GF": 0, "GA": 0, "Pts": 0}
    return stats[t]

for h, x, y, a in matches:
    sh, sa = get(h), get(a)
    sh["P"] += 1; sa["P"] += 1
    sh["GF"] += x; sh["GA"] += y
    sa["GF"] += y; sa["GA"] += x
    if x > y:
        sh["W"] += 1; sa["L"] += 1; sh["Pts"] += 3
    elif x < y:
        sa["W"] += 1; sh["L"] += 1; sa["Pts"] += 3
    else:
        sh["D"] += 1; sa["D"] += 1; sh["Pts"] += 1; sa["Pts"] += 1

def key(t):
    s = stats[t]
    return (-s["Pts"], -(s["GF"] - s["GA"]), -s["GF"])

groups = {}
for t in stats:
    groups.setdefault(key(t), []).append(t)

order = []
for k in sorted(groups):
    g = groups[k]
    if len(g) == 1:
        order.extend(g)
        continue
    gs = set(g)
    h2h = {t: 0 for t in g}
    for h, x, y, a in matches:
        if h in gs and a in gs:
            if x > y:
                h2h[h] += 3
            elif x < y:
                h2h[a] += 3
            else:
                h2h[h] += 1; h2h[a] += 1
    order.extend(sorted(g, key=lambda t: (-h2h[t], t)))

print(f"{'Pos':>3} {'Team':<6} {'P':>2} {'W':>2} {'D':>2} {'L':>2} {'GF':>3} {'GA':>3} {'GD':>3} {'Pts':>3}")
for i, t in enumerate(order, 1):
    s = stats[t]
    gd = s["GF"] - s["GA"]
    gds = f"+{gd}" if gd > 0 else str(gd)
    print(f"{i:>3} {t:<6} {s['P']:>2} {s['W']:>2} {s['D']:>2} {s['L']:>2} {s['GF']:>3} {s['GA']:>3} {gds:>3} {s['Pts']:>3}")
