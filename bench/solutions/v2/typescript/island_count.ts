const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

const grid = lines;
const h = grid.length, w = h > 0 ? grid[0].length : 0;
const seen: boolean[] = new Array(h * w).fill(false);
let islands = 0, largest = 0;
for (let r = 0; r < h; r++) {
  for (let c = 0; c < w; c++) {
    if (grid[r][c] === "#" && !seen[r * w + c]) {
      islands += 1;
      let size = 0;
      const stack = [r * w + c];
      seen[r * w + c] = true;
      while (stack.length > 0) {
        const cur = stack.pop() as number;
        size += 1;
        const y = Math.floor(cur / w), x = cur % w;
        for (let dy = -1; dy <= 1; dy++) {
          for (let dx = -1; dx <= 1; dx++) {
            const ny = y + dy, nx = x + dx;
            if (ny >= 0 && ny < h && nx >= 0 && nx < w && grid[ny][nx] === "#" && !seen[ny * w + nx]) {
              seen[ny * w + nx] = true;
              stack.push(ny * w + nx);
            }
          }
        }
      }
      largest = Math.max(largest, size);
    }
  }
}
console.log(`islands=${islands} largest=${largest}`);
