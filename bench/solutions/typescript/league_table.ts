const results = "Ash 2-1 Bay; Cove 2-0 Ash; Dale 2-2 Ash; Ash 0-2 Elm; Fir 0-0 Ash; Bay 3-2 Cove; Dale 0-2 Bay; Elm 1-2 Bay; Bay 2-2 Fir; Cove 1-3 Dale; Cove 2-0 Elm; Fir 1-0 Cove; Elm 0-0 Dale; Dale 2-2 Fir; Elm 3-2 Fir".split("; ");

type Match = { h: string; a: string; x: number; y: number };
const matches: Match[] = results.map((s) => {
  const [h, score, a] = s.split(" ");
  const [x, y] = score.split("-").map(Number);
  return { h, a, x, y };
});

type Row = { team: string; p: number; w: number; d: number; l: number; gf: number; ga: number; pts: number };
const table = new Map<string, Row>();
const row = (t: string) => {
  if (!table.has(t)) table.set(t, { team: t, p: 0, w: 0, d: 0, l: 0, gf: 0, ga: 0, pts: 0 });
  return table.get(t)!;
};

function record(r: Row, f: number, a: number) {
  r.p++;
  r.gf += f;
  r.ga += a;
  if (f > a) {
    r.w++;
    r.pts += 3;
  } else if (f === a) {
    r.d++;
    r.pts += 1;
  } else r.l++;
}

for (const m of matches) {
  record(row(m.h), m.x, m.y);
  record(row(m.a), m.y, m.x);
}

const key = (r: Row) => [r.pts, r.gf - r.ga, r.gf];
const sameKey = (a: Row, b: Row) => key(a).every((v, i) => v === key(b)[i]);
const teams = [...table.values()];
const h2h = new Map<string, number>();
for (const t of teams) {
  const group = new Set(teams.filter((u) => sameKey(u, t)).map((u) => u.team));
  let pts = 0;
  for (const m of matches) {
    if (!group.has(m.h) || !group.has(m.a)) continue;
    if (m.h === t.team) pts += m.x > m.y ? 3 : m.x === m.y ? 1 : 0;
    if (m.a === t.team) pts += m.y > m.x ? 3 : m.x === m.y ? 1 : 0;
  }
  h2h.set(t.team, pts);
}

teams.sort((a, b) => {
  const ka = key(a);
  const kb = key(b);
  for (let i = 0; i < 3; i++) if (ka[i] !== kb[i]) return kb[i] - ka[i];
  const ha = h2h.get(a.team)!;
  const hb = h2h.get(b.team)!;
  if (ha !== hb) return hb - ha;
  return a.team < b.team ? -1 : a.team > b.team ? 1 : 0;
});

const fmt = (c: string[]) =>
  [c[0].padStart(3), c[1].padEnd(6), ...c.slice(2, 6).map((s) => s.padStart(2)), ...c.slice(6).map((s) => s.padStart(3))].join(" ");
console.log(fmt(["Pos", "Team", "P", "W", "D", "L", "GF", "GA", "GD", "Pts"]));
teams.forEach((r, i) => {
  const gd = r.gf - r.ga;
  console.log(fmt([String(i + 1), r.team, String(r.p), String(r.w), String(r.d), String(r.l), String(r.gf), String(r.ga), gd > 0 ? `+${gd}` : String(gd), String(r.pts)]));
});
