const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

for (const raw of lines) {
  if (raw.replace(/^ +| +$/g, "") === "") continue;
  const s = raw.replace(/ /g, "").replace(/-/g, "");
  if (s.length < 13 || s.length > 19 || !/^[0-9]+$/.test(s)) {
    console.log("bad format");
    continue;
  }
  let total = 0;
  const digits = s.split("").reverse();
  digits.forEach((c: string, i: number) => {
    let d = parseInt(c, 10);
    if (i % 2 === 1) {
      d *= 2;
      if (d > 9) d -= 9;
    }
    total += d;
  });
  console.log(total % 10 === 0 ? "valid" : "bad checksum");
}
