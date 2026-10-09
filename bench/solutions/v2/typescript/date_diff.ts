const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

const isLeap = (y: number): boolean => y % 4 === 0 && (y % 100 !== 0 || y % 400 === 0);
const monthDays = (y: number, m: number): number => [31, isLeap(y) ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][m - 1];

// days since 0001-01-01 of a date text, or -1
function dayNumber(token: string): number {
  if (!/^[0-9]{4}-[0-9]{2}-[0-9]{2}$/.test(token)) return -1;
  const y = parseInt(token.slice(0, 4), 10), m = parseInt(token.slice(5, 7), 10), d = parseInt(token.slice(8, 10), 10);
  if (y < 1 || m < 1 || m > 12 || d < 1 || d > monthDays(y, m)) return -1;
  let total = (y - 1) * 365 + Math.floor((y - 1) / 4) - Math.floor((y - 1) / 100) + Math.floor((y - 1) / 400);
  for (let k = 1; k < m; k++) total += monthDays(y, k);
  return total + d;
}

for (const line of lines) {
  const parts = line.split(" ");
  if (parts.length !== 2) {
    console.log("invalid");
    continue;
  }
  const a = dayNumber(parts[0]), b = dayNumber(parts[1]);
  console.log(a < 0 || b < 0 ? "invalid" : String(b - a));
}
