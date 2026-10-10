const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

const split = lines.indexOf("");
const drawing = lines.slice(0, split);
const stackCount = drawing[drawing.length - 1].split(" ").filter((t: string) => t !== "").length;
const base: string[][] = [];
for (let k = 0; k < stackCount; k++) base.push([]);
for (let row = drawing.length - 2; row >= 0; row--) {
  for (let k = 0; k < stackCount; k++) {
    const at = 1 + 4 * k;
    if (at < drawing[row].length && drawing[row][at] !== " ") base[k].push(drawing[row][at]);
  }
}
const one = base.map((s: string[]) => s.slice());
const many = base.map((s: string[]) => s.slice());
lines.forEach((line: string, i: number) => {
  if (i <= split || line === "") return;
  const words = line.split(" ");
  const count = parseInt(words[1], 10), src = parseInt(words[3], 10), dst = parseInt(words[5], 10);
  if (src < 1 || src > stackCount || dst < 1 || dst > stackCount || count > one[src - 1].length) {
    console.log(`line ${i + 1}: impossible move`);
    return;
  }
  for (let j = 0; j < count; j++) one[dst - 1].push(one[src - 1].pop() as string);
  const lifted = many[src - 1].splice(many[src - 1].length - count, count);
  many[dst - 1].push(...lifted);
});
const tops = (stacks: string[][]): string => stacks.map((s: string[]) => (s.length > 0 ? s[s.length - 1] : "-")).join("");
console.log("one at a time: " + tops(one));
console.log("all at once: " + tops(many));
