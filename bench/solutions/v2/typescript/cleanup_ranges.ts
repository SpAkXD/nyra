const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

let contained = 0, overlapping = 0;
lines.forEach((line: string, i: number) => {
  if (line === "") return;
  const m = /^([0-9]+)-([0-9]+),([0-9]+)-([0-9]+)$/.exec(line);
  if (m === null) {
    console.log(`line ${i + 1}: invalid`);
    return;
  }
  const [a, b, c, d] = m.slice(1).map((g: string) => parseInt(g, 10));
  if (a > b || c > d) {
    console.log(`line ${i + 1}: invalid`);
    return;
  }
  if ((a <= c && d <= b) || (c <= a && b <= d)) contained += 1;
  if (a <= d && c <= b) overlapping += 1;
});
console.log(`contained=${contained} overlapping=${overlapping}`);
