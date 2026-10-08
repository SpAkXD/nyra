const start = ["#......#", "..##....", ".#..#...", "..##....", "........", "#.....##"];
const R = 6;
const C = 8;
let g: number[][] = start.map((row) => [...row].map((ch) => (ch === "#" ? 1 : 0)));
for (let gen = 1; gen <= 7; gen++) {
  const next: number[][] = [];
  for (let r = 0; r < R; r++) {
    next.push([]);
    for (let c = 0; c < C; c++) {
      let n = 0;
      for (let dr = -1; dr <= 1; dr++) {
        for (let dc = -1; dc <= 1; dc++) {
          if (dr === 0 && dc === 0) continue;
          if (g[(r + dr + R) % R][(c + dc + C) % C] > 0) n++;
        }
      }
      const age = g[r][c];
      if (age === 0) next[r].push(n === 3 ? 1 : 0);
      else if (n === 2 || n === 3) next[r].push(age + 1 === 5 ? 0 : age + 1);
      else next[r].push(0);
    }
  }
  g = next;
  let p = 0;
  let oldest = 0;
  for (const row of g) for (const a of row) if (a > 0) { p++; oldest = Math.max(oldest, a); }
  console.log(`gen ${gen}: ${p} alive, oldest ${oldest}`);
}
for (const row of g) console.log(row.map((a) => (a > 0 ? String(a) : ".")).join(""));
