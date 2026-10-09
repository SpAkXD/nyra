const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

const after = new Map<string, string[]>();
const indegree = new Map<string, number>();
for (const line of lines) {
  if (line === "") continue;
  const [a, b] = line.split(" -> ");
  if (!indegree.has(a)) indegree.set(a, 0);
  if (!indegree.has(b)) indegree.set(b, 0);
  if (!after.has(a)) after.set(a, []);
  (after.get(a) as string[]).push(b);
  indegree.set(b, (indegree.get(b) as number) + 1);
}
const ready: string[] = Array.from(indegree.keys()).filter((t: string) => indegree.get(t) === 0);
const order: string[] = [];
while (ready.length > 0) {
  ready.sort((a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0));
  const t = ready.shift() as string;
  order.push(t);
  for (const u of after.get(t) || []) {
    indegree.set(u, (indegree.get(u) as number) - 1);
    if (indegree.get(u) === 0) ready.push(u);
  }
}
console.log(order.length === indegree.size ? order.join(" ") : "cycle");
