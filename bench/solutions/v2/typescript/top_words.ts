const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

const k = parseInt(lines[0], 10);
const counts = new Map<string, number>();
for (const word of lines.slice(1).join("\n").match(/[A-Za-z]+/g) || []) {
  const w = word.toLowerCase();
  counts.set(w, (counts.get(w) || 0) + 1);
}
if (counts.size === 0) console.log("no words");
const ranked = Array.from(counts.entries()).sort((a, b) => b[1] - a[1] || (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0));
for (const [word, n] of ranked.slice(0, k)) console.log(`${word} ${n}`);
