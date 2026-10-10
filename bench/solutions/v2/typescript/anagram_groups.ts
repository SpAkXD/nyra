const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

const lower = (s: string): string => s.toLowerCase();
const cmp = (a: string, b: string): number => (a < b ? -1 : a > b ? 1 : 0);
const seen = new Set<string>();
const groups = new Map<string, string[]>();
for (const raw of lines) {
  const word = raw.replace(/^ +| +$/g, "");
  if (word === "" || seen.has(lower(word))) continue;
  seen.add(lower(word));
  const key = lower(word).split("").sort().join("");
  if (!groups.has(key)) groups.set(key, []);
  (groups.get(key) as string[]).push(word);
}
const result = Array.from(groups.values()).filter((w: string[]) => w.length >= 2)
  .map((w: string[]) => w.slice().sort((a: string, b: string) => cmp(lower(a), lower(b))));
result.sort((a: string[], b: string[]) => cmp(lower(a[0]), lower(b[0])));
if (result.length > 0) for (const words of result) console.log(words.join(" "));
else console.log("no anagrams");
