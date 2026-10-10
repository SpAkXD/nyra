const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

const steps = parseInt(input.split(/\s+/).filter((t: string) => t !== "")[0], 10);
const black = new Set<string>();
let x = 0, y = 0, facing = 0;
const dx = [0, 1, 0, -1], dy = [-1, 0, 1, 0];
for (let i = 0; i < steps; i++) {
  const key = `${x},${y}`;
  if (black.has(key)) {
    facing = (facing + 3) % 4;
    black.delete(key);
  } else {
    facing = (facing + 1) % 4;
    black.add(key);
  }
  x += dx[facing];
  y += dy[facing];
}
console.log(`black=${black.size} x=${x} y=${y} facing=${"URDL"[facing]}`);
