const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

const totals: number[] = [];
let current = -1;
lines.forEach((line: string, i: number) => {
  if (line === "") {
    if (current >= 0) {
      totals.push(current);
      current = -1;
    }
  } else if (/^[0-9]+$/.test(line)) {
    current = (current < 0 ? 0 : current) + parseInt(line, 10);
  } else {
    console.log(`line ${i + 1}: invalid`);
  }
});
if (current >= 0) totals.push(current);
totals.sort((a: number, b: number) => b - a);
console.log(`hikers=${totals.length}`);
console.log(`top1=${totals.length > 0 ? totals[0] : 0}`);
console.log(`top3=${totals.slice(0, 3).reduce((a: number, b: number) => a + b, 0)}`);
