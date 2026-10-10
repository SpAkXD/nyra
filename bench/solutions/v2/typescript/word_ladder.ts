const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

const [start, end] = lines[0].split(" ");
const words = Array.from(new Set(lines.slice(1).filter((w: string) => w !== ""))).sort((a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0));
const near = (a: string, b: string): boolean => {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) diff++;
  return diff === 1;
};
if (start === end) {
  console.log(`1 ${start}`);
} else if (!words.includes(end)) {
  console.log("no ladder");
} else {
  const nodes = [start, ...words.filter((w: string) => w !== start)];
  const dist: number[] = new Array(nodes.length).fill(-1);
  const goal = nodes.indexOf(end);
  dist[goal] = 0;
  const queue = [goal];
  for (let head = 0; head < queue.length; head++) {
    const u = queue[head];
    for (let v = 0; v < nodes.length; v++) {
      if (dist[v] < 0 && near(nodes[u], nodes[v])) {
        dist[v] = dist[u] + 1;
        queue.push(v);
      }
    }
  }
  if (dist[0] < 0) {
    console.log("no ladder");
  } else {
    const path = [start];
    let cur = 0;
    while (cur !== goal) {
      let best = "", bestAt = -1;
      for (let v = 1; v < nodes.length; v++) {
        if (dist[v] === dist[cur] - 1 && near(nodes[cur], nodes[v]) && (bestAt < 0 || nodes[v] < best)) {
          best = nodes[v];
          bestAt = v;
        }
      }
      path.push(best);
      cur = bestAt;
    }
    console.log(`${path.length} ` + path.join(" "));
  }
}
