const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

const PATTERN = /^[0-9]{4}-([0-9]{2})-([0-9]{2}) ([0-9]{2}):([0-9]{2}):([0-9]{2}) (DEBUG|INFO|WARN|ERROR) ([\s\S]+)$/;
const counts: { [level: string]: number } = { DEBUG: 0, INFO: 0, WARN: 0, ERROR: 0 };
let last = "none";
lines.forEach((line: string, i: number) => {
  if (line === "") return;
  const m = PATTERN.exec(line);
  if (m !== null) {
    const month = parseInt(m[1], 10), day = parseInt(m[2], 10);
    const hour = parseInt(m[3], 10), minute = parseInt(m[4], 10), second = parseInt(m[5], 10);
    if (month >= 1 && month <= 12 && day >= 1 && day <= 31 && hour <= 23 && minute <= 59 && second <= 59) {
      counts[m[6]] += 1;
      if (m[6] === "ERROR") last = m[7];
      return;
    }
  }
  console.log(`line ${i + 1}: malformed`);
});
for (const level of ["DEBUG", "INFO", "WARN", "ERROR"]) console.log(`${level}: ${counts[level]}`);
console.log(`last error: ${last}`);
