const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

const expand = (s: string, left: number, right: number): number => {
  let lo = left, hi = right;
  while (lo >= 0 && hi < s.length && s[lo] === s[hi]) {
    lo -= 1;
    hi += 1;
  }
  return hi - lo - 1;
};
for (const line of lines) {
  if (line === "") {
    console.log("(empty)");
    continue;
  }
  let bestStart = 0, bestLen = 1;
  for (let center = 0; center < line.length; center++) {
    for (let kind = 0; kind < 2; kind++) {
      const length = expand(line, center, center + kind);
      const start = kind === 0 ? center - Math.floor((length - 1) / 2) : center - Math.floor(length / 2) + 1;
      if (length > bestLen || (length === bestLen && start < bestStart)) {
        bestStart = start;
        bestLen = length;
      }
    }
  }
  console.log(line.slice(bestStart, bestStart + bestLen));
}
