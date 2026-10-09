const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

const tokens = input.split(/\s+/).filter((t: string) => t !== "").map((t: string) => parseInt(t, 10));
const n = tokens[0], capacity = tokens[1];
const value: number[] = new Array(capacity + 1).fill(0);
const count: number[] = new Array(capacity + 1).fill(0);
for (let i = 0; i < n; i++) {
  const weight = tokens[2 + 2 * i], worth = tokens[3 + 2 * i];
  for (let w = capacity; w >= weight; w--) {
    const v = value[w - weight] + worth, k = count[w - weight] + 1;
    if (v > value[w] || (v === value[w] && k < count[w])) {
      value[w] = v;
      count[w] = k;
    }
  }
}
console.log(`best=${value[capacity]}`);
console.log(`taken=${count[capacity]}`);
