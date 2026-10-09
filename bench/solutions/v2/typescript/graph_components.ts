const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

const tokens = input.split(/\s+/).filter((t: string) => t !== "").map((t: string) => parseInt(t, 10));
const n = tokens[0], m = tokens[1];
const parent: number[] = [];
for (let i = 0; i <= n; i++) parent.push(i);
const find = (x: number): number => {
  while (parent[x] !== x) {
    parent[x] = parent[parent[x]];
    x = parent[x];
  }
  return x;
};
for (let i = 0; i < m; i++) parent[find(tokens[2 + 2 * i])] = find(tokens[3 + 2 * i]);
const size: { [root: number]: number } = {};
const smallest: { [root: number]: number } = {};
for (let node = 1; node <= n; node++) {
  const root = find(node);
  size[root] = (size[root] || 0) + 1;
  if (smallest[root] === undefined) smallest[root] = node;
}
const roots = Object.keys(size).map((k: string) => parseInt(k, 10));
roots.sort((a: number, b: number) => size[b] - size[a] || smallest[a] - smallest[b]);
console.log(`components=${roots.length}`);
console.log("sizes: " + roots.map((r: number) => String(size[r])).join(" "));
console.log(`largest contains node ${smallest[roots[0]]}`);
