const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

const priority = (c: string): number => (c >= "a" ? c.charCodeAt(0) - 96 : c.charCodeAt(0) - 38);
const lowestCommon = (texts: string[]): number => {
  const sets = texts.map((t: string) => new Set(t.split("")));
  let best = 0;
  for (const c of Array.from(sets[0])) if (sets.every((s: Set<string>) => s.has(c)) && (best === 0 || priority(c) < best)) best = priority(c);
  return best;
};
const valid: string[] = [];
let shared = 0;
lines.forEach((line: string, i: number) => {
  if (line === "") return;
  if (line.length % 2 !== 0 || !/^[A-Za-z]+$/.test(line)) {
    console.log(`line ${i + 1}: invalid`);
    return;
  }
  valid.push(line);
  const half = line.length / 2;
  shared += lowestCommon([line.slice(0, half), line.slice(half)]);
});
let badges = 0;
for (let i = 0; i + 3 <= valid.length; i += 3) badges += lowestCommon([valid[i], valid[i + 1], valid[i + 2]]);
console.log(`shared=${shared}`);
console.log(`badges=${badges}`);
