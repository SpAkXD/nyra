const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

const PATTERN = /^([0-9]{1,4}):([0-9]{2}):([0-9]{2})$/;
let total = 0;
let valid = 0;
lines.forEach((raw: string, i: number) => {
  const s = raw.replace(/^ +| +$/g, "");
  if (s === "") return;
  const m = PATTERN.exec(s);
  if (m !== null && parseInt(m[2], 10) < 60 && parseInt(m[3], 10) < 60) {
    total += parseInt(m[1], 10) * 3600 + parseInt(m[2], 10) * 60 + parseInt(m[3], 10);
    valid += 1;
  } else {
    console.log(`line ${i + 1}: invalid`);
  }
});
const two = (n: number): string => String(n).padStart(2, "0");
console.log(`total=${two(Math.floor(total / 3600))}:${two(Math.floor((total % 3600) / 60))}:${two(total % 60)} valid=${valid}`);
