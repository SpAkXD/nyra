const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

const coins = lines[0].split(" ").map((t: string) => parseInt(t, 10)).sort((a: number, b: number) => b - a);
for (const line of lines.slice(1)) {
  const target = parseInt(line, 10);
  const none = target + 1;
  const fewest: number[] = new Array(target + 1).fill(none);
  fewest[0] = 0;
  for (let amount = 1; amount <= target; amount++) {
    for (const c of coins) if (c <= amount && fewest[amount - c] + 1 < fewest[amount]) fewest[amount] = fewest[amount - c] + 1;
  }
  if (fewest[target] === none) {
    console.log("impossible");
    continue;
  }
  const used: number[] = [];
  let left = target;
  while (left > 0) {
    for (const c of coins) {
      if (c <= left && fewest[left - c] === fewest[left] - 1) {
        used.push(c);
        left -= c;
        break;
      }
    }
  }
  console.log(`${fewest[target]}:` + used.map((c: number) => ` ${c}`).join(""));
}
