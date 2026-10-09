const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

const [rows, cols] = lines[0].split(" ").map((t: string) => parseInt(t, 10));
const MOD = 1000000007;
const ways: number[] = new Array(rows * cols).fill(0);
for (let r = 0; r < rows; r++) {
  for (let c = 0; c < cols; c++) {
    if (lines[1 + r][c] === "#") continue;
    if (r === 0 && c === 0) ways[0] = 1;
    else {
      const up = r > 0 ? ways[(r - 1) * cols + c] : 0;
      const left = c > 0 ? ways[r * cols + c - 1] : 0;
      ways[r * cols + c] = (up + left) % MOD;
    }
  }
}
console.log(String(ways[rows * cols - 1]));
