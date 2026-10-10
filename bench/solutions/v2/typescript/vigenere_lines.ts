const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

const key = lines[0].split("").map((c: string) => c.charCodeAt(0) - 97);
for (const line of lines.slice(1)) {
  if (line.length < 2 || (line[0] !== "E" && line[0] !== "D") || line[1] !== " ") {
    console.log("error");
    continue;
  }
  const sign = line[0] === "E" ? 1 : -1;
  let out = "";
  let i = 0;
  for (const c of line.slice(2)) {
    if (/^[A-Za-z]$/.test(c)) {
      const base = c >= "a" ? 97 : 65;
      const shifted = (((c.charCodeAt(0) - base + sign * key[i % key.length]) % 26) + 26) % 26;
      out += String.fromCharCode(shifted + base);
      i += 1;
    } else {
      out += c;
    }
  }
  console.log(out);
}
