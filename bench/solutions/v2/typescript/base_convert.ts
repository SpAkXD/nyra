const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

const DIGITS = "0123456789abcdefghijklmnopqrstuvwxyz";
const baseOk = (t: string): boolean => /^[0-9]{1,2}$/.test(t) && parseInt(t, 10) >= 2 && parseInt(t, 10) <= 36;

function toBase(n: number, base: number): string {
  if (n === 0) return "0";
  let out = "";
  let m = Math.abs(n);
  while (m > 0) {
    out = DIGITS[m % base] + out;
    m = Math.floor(m / base);
  }
  return (n < 0 ? "-" : "") + out;
}

for (const line of lines) {
  const parts = line.split(" ");
  if (parts.length !== 3 || !baseOk(parts[1]) || !baseOk(parts[2])) {
    console.log("invalid");
    continue;
  }
  const number = parts[0], src = parseInt(parts[1], 10), dst = parseInt(parts[2], 10);
  const negative = number.startsWith("-");
  const body = (negative ? number.slice(1) : number).toLowerCase();
  let value = 0;
  let ok = body !== "";
  for (const c of body) {
    const d = DIGITS.indexOf(c);
    if (d < 0 || d >= src) {
      ok = false;
      break;
    }
    value = value * src + d;
  }
  console.log(ok ? toBase(negative ? -value : value, dst) : "invalid");
}
