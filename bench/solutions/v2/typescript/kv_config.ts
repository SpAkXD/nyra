const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

let section = "";
const conf = new Map<string, string>();
let errors = 0;
const trim = (s: string): string => s.replace(/^[ \t]+|[ \t]+$/g, "");
lines.forEach((raw: string, i: number) => {
  const line = trim(raw);
  const number = i + 1;
  if (line === "" || line.startsWith("#")) return;
  if (line.startsWith("[") && line.endsWith("]")) {
    const name = trim(line.slice(1, line.length - 1));
    if (name === "") {
      console.log(`line ${number}: syntax error`);
      errors += 1;
    } else {
      section = name;
    }
    return;
  }
  const eq = line.indexOf("=");
  if (eq >= 0) {
    const key = trim(line.slice(0, eq));
    const value = trim(line.slice(eq + 1));
    if (key !== "") {
      conf.set(section ? section + "." + key : key, value);
      return;
    }
  }
  console.log(`line ${number}: syntax error`);
  errors += 1;
});
const keys = Array.from(conf.keys()).sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
for (const k of keys) console.log(`${k}=${conf.get(k)}`);
console.log(`entries=${conf.size} errors=${errors}`);
