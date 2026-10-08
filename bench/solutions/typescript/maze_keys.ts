const mazes: string[][] = [
  ["S...#", ".##.#", "....G"],
  ["#########", "#S..A..G#", "#.#####.#", "#...a.#.#", "#########"],
  ["S.B.G", "#.###", "b.A.a"],
  ["#######", "#a#.#b#", "#.#.#.#", "#..S..#", "###A###", "###B###", "###G###"],
  ["S.#..", ".##.#", "...#G"],
  ["..S..", ".###.", "..G.."],
];

const moves: [string, number, number][] = [["D", 1, 0], ["L", 0, -1], ["R", 0, 1], ["U", -1, 0]];

function solve(g: string[]): string | null {
  const h = g.length;
  const w = g[0].length;
  let sr = 0;
  let sc = 0;
  for (let r = 0; r < h; r++) for (let c = 0; c < w; c++) if (g[r][c] === "S") [sr, sc] = [r, c];
  const seen = new Set<string>();
  const start = { r: sr, c: sc, keys: "", path: "" };
  seen.add(`${sr},${sc},`);
  let queue = [start];
  while (queue.length > 0) {
    const next: typeof queue = [];
    for (const s of queue) {
      for (const [m, dr, dc] of moves) {
        const r = s.r + dr;
        const c = s.c + dc;
        if (r < 0 || r >= h || c < 0 || c >= w) continue;
        const ch = g[r][c];
        if (ch === "#") continue;
        if (ch >= "A" && ch <= "Z" && ch !== "S" && ch !== "G" && !s.keys.includes(ch.toLowerCase())) continue;
        if (ch === "G") return s.path + m;
        let keys = s.keys;
        if (ch >= "a" && ch <= "z" && !keys.includes(ch)) keys = [...keys, ch].sort().join("");
        const id = `${r},${c},${keys}`;
        if (seen.has(id)) continue;
        seen.add(id);
        next.push({ r, c, keys, path: s.path + m });
      }
    }
    queue = next;
  }
  return null;
}

mazes.forEach((g, i) => {
  const p = solve(g);
  console.log(p === null ? `maze ${i + 1}: no path` : `maze ${i + 1}: ${p.length} moves ${p}`);
});
