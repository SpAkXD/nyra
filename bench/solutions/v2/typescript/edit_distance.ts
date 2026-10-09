const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

function distance(a: string, b: string): number {
  let previous: number[] = [];
  for (let j = 0; j <= b.length; j++) previous.push(j);
  for (let i = 1; i <= a.length; i++) {
    const row = [i];
    for (let j = 1; j <= b.length; j++) {
      row.push(Math.min(previous[j] + 1, row[j - 1] + 1, previous[j - 1] + (a[i - 1] !== b[j - 1] ? 1 : 0)));
    }
    previous = row;
  }
  return previous[b.length];
}

for (const line of lines) {
  const parts = line.split("|");
  console.log(parts.length === 2 ? String(distance(parts[0], parts[1])) : "invalid");
}
