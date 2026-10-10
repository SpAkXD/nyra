const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

const PAIRS: Array<[number, string]> = [[1000, "M"], [900, "CM"], [500, "D"], [400, "CD"], [100, "C"], [90, "XC"], [50, "L"],
  [40, "XL"], [10, "X"], [9, "IX"], [5, "V"], [4, "IV"], [1, "I"]];
const LETTER: { [c: string]: number } = { I: 1, V: 5, X: 10, L: 50, C: 100, D: 500, M: 1000 };

function toRoman(n: number): string {
  let out = "";
  for (const [value, symbol] of PAIRS) {
    while (n >= value) {
      out += symbol;
      n -= value;
    }
  }
  return out;
}

function fromRoman(s: string): number {
  let total = 0;
  for (let i = 0; i < s.length; i++) {
    const v = LETTER[s[i]];
    if (i + 1 < s.length && v < LETTER[s[i + 1]]) total -= v;
    else total += v;
  }
  return total;
}

for (const raw of lines) {
  const s = raw.replace(/^ +| +$/g, "");
  if (s === "") continue;
  if (/^[0-9]+$/.test(s)) {
    const n = parseInt(s, 10);
    console.log(n >= 1 && n <= 3999 ? toRoman(n) : "INVALID");
  } else if (/^[IVXLCDM]+$/.test(s)) {
    const n = fromRoman(s);
    console.log(n >= 1 && n <= 3999 && toRoman(n) === s ? String(n) : "INVALID");
  } else {
    console.log("INVALID");
  }
}
