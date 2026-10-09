const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

const blocks: string[][] = [];
let current: string[] = [];
for (const line of lines) {
  if (line === "") {
    if (current.length > 0) {
      blocks.push(current);
      current = [];
    }
  } else {
    current.push(line);
  }
}
if (current.length > 0) blocks.push(current);

blocks.forEach((grid: string[], k: number) => {
  const h = grid.length, w = grid[0].length;
  let start = -1, end = -1;
  for (let r = 0; r < h; r++) {
    for (let c = 0; c < w; c++) {
      if (grid[r][c] === "S") start = r * w + c;
      else if (grid[r][c] === "E") end = r * w + c;
    }
  }
  const dist: number[] = new Array(h * w).fill(-1);
  dist[start] = 0;
  const queue = [start];
  for (let head = 0; head < queue.length; head++) {
    const cur = queue[head];
    const r = Math.floor(cur / w), c = cur % w;
    for (const [dr, dc] of [[1, 0], [-1, 0], [0, 1], [0, -1]]) {
      const nr = r + dr, nc = c + dc;
      if (nr >= 0 && nr < h && nc >= 0 && nc < w && grid[nr][nc] !== "#" && dist[nr * w + nc] < 0) {
        dist[nr * w + nc] = dist[cur] + 1;
        queue.push(nr * w + nc);
      }
    }
  }
  console.log(dist[end] >= 0 ? `Maze ${k + 1}: ${dist[end]}` : `Maze ${k + 1}: no path`);
});
