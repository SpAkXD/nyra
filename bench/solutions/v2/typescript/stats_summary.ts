const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

const values: number[] = [];
lines.forEach((raw: string, i: number) => {
  const s = raw.replace(/^ +| +$/g, "");
  if (s === "") return;
  if (/^-?[0-9]+(\.[0-9]+)?$/.test(s)) values.push(parseFloat(s));
  else console.log(`line ${i + 1}: not a number`);
});
if (values.length === 0) {
  console.log("no data");
} else {
  const n = values.length;
  let total = 0;
  for (const v of values) total += v;
  const mean = total / n;
  const ordered = values.slice().sort((a: number, b: number) => a - b);
  const median = n % 2 === 1 ? ordered[(n - 1) / 2] : (ordered[n / 2 - 1] + ordered[n / 2]) / 2;
  let squares = 0;
  for (const v of values) squares += (v - mean) * (v - mean);
  const stddev = Math.sqrt(squares / n);
  console.log(`count=${n}`);
  console.log(`mean=${mean.toFixed(3)}`);
  console.log(`median=${median.toFixed(3)}`);
  console.log(`stddev=${stddev.toFixed(3)}`);
  console.log(`range=${ordered[0].toFixed(3)}..${ordered[n - 1].toFixed(3)}`);
}
