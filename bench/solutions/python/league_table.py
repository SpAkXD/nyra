results = ("Ash 2-1 Bay; Cove 2-0 Ash; Dale 2-2 Ash; Ash 0-2 Elm; Fir 0-0 Ash; Bay 3-2 Cove; Dale 0-2 Bay; "
           "Elm 1-2 Bay; Bay 2-2 Fir; Cove 1-3 Dale; Cove 2-0 Elm; Fir 1-0 Cove; Elm 0-0 Dale; Dale 2-2 Fir; "
           "Elm 3-2 Fir")

matches = []
for text in results.split("; "):
    home, score, away = text.split(" ")
    x, y = (int(v) for v in score.split("-"))
    matches.append((home, x, y, away))
teams = sorted({m[0] for m in matches} | {m[3] for m in matches})


def points(f, g):
    return 3 if f > g else 1 if f == g else 0


table = {t: {"P": 0, "W": 0, "D": 0, "L": 0, "GF": 0, "GA": 0, "Pts": 0} for t in teams}
for home, x, y, away in matches:
    for team, f, g in ((home, x, y), (away, y, x)):
        row = table[team]
        row["P"] += 1
        row["GF"] += f
        row["GA"] += g
        row["W" if f > g else "D" if f == g else "L"] += 1
        row["Pts"] += points(f, g)


def basic(team):
    row = table[team]
    return (row["Pts"], row["GF"] - row["GA"], row["GF"])


ranked = []
for key in sorted({basic(t) for t in teams}, reverse=True):
    group = [t for t in teams if basic(t) == key]
    h2h = {t: 0 for t in group}
    for home, x, y, away in matches:
        if home in group and away in group:
            h2h[home] += points(x, y)
            h2h[away] += points(y, x)
    ranked += sorted(group, key=lambda t: (-h2h[t], t))

print(f"{'Pos':>3} {'Team':<6} {'P':>2} {'W':>2} {'D':>2} {'L':>2} {'GF':>3} {'GA':>3} {'GD':>3} {'Pts':>3}")
for pos, team in enumerate(ranked, 1):
    r = table[team]
    gd = r["GF"] - r["GA"]
    gd_text = f"+{gd}" if gd > 0 else str(gd)
    print(f"{pos:>3} {team:<6} {r['P']:>2} {r['W']:>2} {r['D']:>2} {r['L']:>2} {r['GF']:>3} {r['GA']:>3} "
          f"{gd_text:>3} {r['Pts']:>3}")
