const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

const xs: number[] = [], ys: number[] = [];
lines.forEach((raw: string, i: number) => {
  if (raw === "") return;
  const parts = raw.split(" ");
  if (parts.length === 2 && parts.every((p: string) => /^-?[0-9]+(\.[0-9]+)?$/.test(p))) {
    xs.push(parseFloat(parts[0]));
    ys.push(parseFloat(parts[1]));
  } else {
    console.log(`line ${i + 1}: bad point`);
  }
});
const n = xs.length;
let sx = 0, sy = 0, sxy = 0, sxx = 0;
for (let i = 0; i < n; i++) {
  sx += xs[i];
  sy += ys[i];
  sxy += xs[i] * ys[i];
  sxx += xs[i] * xs[i];
}
const denominator = n * sxx - sx * sx;
if (n < 2 || denominator === 0) {
  console.log(`n=${n} undefined`);
} else {
  const a = (n * sxy - sx * sy) / denominator;
  const b = (sy - a * sx) / n;
  console.log(`n=${n} slope=${a.toFixed(4)} intercept=${b.toFixed(4)}`);
}
