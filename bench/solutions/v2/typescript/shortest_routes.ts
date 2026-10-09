const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

const [n, m] = lines[0].split(" ").map((t: string) => parseInt(t, 10));
const adjacent: Array<Array<[number, number]>> = [];
for (let i = 0; i <= n; i++) adjacent.push([]);
for (let i = 0; i < m; i++) {
  const [a, b, d] = lines[1 + i].split(" ").map((t: string) => parseInt(t, 10));
  adjacent[a].push([b, d]);
  adjacent[b].push([a, d]);
}
for (const line of lines.slice(1 + m)) {
  const [s, t] = line.split(" ").map((x: string) => parseInt(x, 10));
  const dist: number[] = new Array(n + 1).fill(-1);
  const done: boolean[] = new Array(n + 1).fill(false);
  dist[s] = 0;
  for (;;) {
    let u = -1;
    for (let v = 1; v <= n; v++) if (!done[v] && dist[v] >= 0 && (u < 0 || dist[v] < dist[u])) u = v;
    if (u < 0) break;
    done[u] = true;
    for (const [v, w] of adjacent[u]) if (dist[v] < 0 || dist[u] + w < dist[v]) dist[v] = dist[u] + w;
  }
  console.log(dist[t] >= 0 ? String(dist[t]) : "unreachable");
}
