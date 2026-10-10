const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

const [w, h] = lines[0].split(" ").map((t: string) => parseInt(t, 10));
const grid = lines.slice(1, 1 + h);
const commands = lines.slice(1 + h).join("");
let x = 0, y = 0;
for (let r = 0; r < h; r++) for (let c = 0; c < w; c++) if (grid[r][c] === "R") { x = c; y = r; }
const dx = [0, 1, 0, -1], dy = [-1, 0, 1, 0];
let facing = 0, blocked = 0;
const visited = new Set<number>([y * 1000 + x]);
for (const c of commands) {
  if (c === "L") facing = (facing + 3) % 4;
  else if (c === "R") facing = (facing + 1) % 4;
  else if (c === "F") {
    const nx = x + dx[facing], ny = y + dy[facing];
    if (nx >= 0 && nx < w && ny >= 0 && ny < h && grid[ny][nx] !== "#") {
      x = nx;
      y = ny;
      visited.add(y * 1000 + x);
    } else {
      blocked += 1;
    }
  }
}
console.log(`x=${x} y=${y} facing=${"NESW"[facing]} blocked=${blocked} visited=${visited.size}`);
