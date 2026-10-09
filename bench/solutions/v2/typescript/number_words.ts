const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

const ONES = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve", "thirteen",
  "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen"];
const TENS = ["", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"];

function belowThousand(n: number): string[] {
  const words: string[] = [];
  if (n >= 100) {
    words.push(ONES[Math.floor(n / 100)], "hundred");
    n %= 100;
  }
  if (n >= 20) words.push(TENS[Math.floor(n / 10)] + (n % 10 ? "-" + ONES[n % 10] : ""));
  else if (n > 0) words.push(ONES[n]);
  return words;
}

function spell(n: number): string {
  if (n === 0) return "zero";
  let words: string[] = [];
  for (const [size, name] of [[1000000, "million"], [1000, "thousand"], [1, ""]] as Array<[number, string]>) {
    const part = Math.floor(n / size);
    n %= size;
    if (part > 0) words = words.concat(belowThousand(part), name ? [name] : []);
  }
  return words.join(" ");
}

for (const raw of lines) {
  const s = raw.replace(/^ +| +$/g, "");
  if (s === "") continue;
  if (!/^-?[0-9]+$/.test(s) || Math.abs(parseInt(s, 10)) > 999999999) {
    console.log("invalid");
    continue;
  }
  const n = parseInt(s, 10);
  console.log((n < 0 ? "minus " : "") + spell(Math.abs(n)));
}
