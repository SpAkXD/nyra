const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();
let total = 0;
let count = 0;
lines.forEach((raw: string, i: number) => {
  const s = raw.replace(/^[ \t]+|[ \t]+$/g, "");
  if (s === "") return;
  if (/^-?[0-9]+$/.test(s)) {
    total += parseInt(s, 10);
    count += 1;
  } else {
    console.log(`line ${i + 1}: invalid`);
  }
});
console.log(`sum=${total} count=${count}`);
