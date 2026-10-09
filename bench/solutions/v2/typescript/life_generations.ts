const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

const [w, h, generations] = lines[0].split(" ").map((t: string) => parseInt(t, 10));
let grid: boolean[][] = [];
for (let r = 0; r < h; r++) grid.push(lines[1 + r].split("").map((c: string) => c === "#"));
for (let g = 0; g < generations; g++) {
  const next: boolean[][] = [];
  for (let r = 0; r < h; r++) {
    const row: boolean[] = [];
    for (let c = 0; c < w; c++) {
      let n = 0;
      for (let dr = -1; dr <= 1; dr++) {
        for (let dc = -1; dc <= 1; dc++) {
          if ((dr !== 0 || dc !== 0) && r + dr >= 0 && r + dr < h && c + dc >= 0 && c + dc < w && grid[r + dr][c + dc]) n++;
        }
      }
      row.push(n === 3 || (grid[r][c] && n === 2));
    }
    next.push(row);
  }
  grid = next;
}
let alive = 0;
for (const row of grid) {
  console.log(row.map((cell: boolean) => (cell ? "#" : ".")).join(""));
  alive += row.filter((cell: boolean) => cell).length;
}
console.log(`alive=${alive}`);
