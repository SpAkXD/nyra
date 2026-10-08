const requests = "ann 4; bob 4; cat 2; dan 10; eve 1; fay 6; cancel bob; gus 5; hal 3; cancel zed; ivy 9; jo 2; kai 8; lea 3".split("; ");
const rows = "ABCDEF";
const order = "CDBEAF";
const broken = new Set(["C5", "C6", "E1"]);
const owner = new Map<string, string>(); // seat -> group

const free = (r: string, s: number) => !broken.has(r + s) && !owner.has(r + s);

for (const req of requests) {
  const [a, b] = req.split(" ");
  if (a === "cancel") {
    let count = 0;
    for (const [seat, g] of [...owner]) {
      if (g === b) {
        owner.delete(seat);
        count++;
      }
    }
    console.log(count > 0 ? `${b} cancelled (${count} seats)` : `${b} has no booking`);
    continue;
  }
  const k = Number(b);
  let done = false;
  for (const r of order) {
    let bestStart = -1;
    let bestDist = Infinity;
    for (let s = 1; s + k - 1 <= 10; s++) {
      let ok = true;
      for (let j = s; j < s + k; j++) if (!free(r, j)) ok = false;
      if (!ok) continue;
      const dist = Math.abs((s + s + k - 1) / 2 - 5.5);
      if (dist < bestDist) {
        bestDist = dist;
        bestStart = s;
      }
    }
    if (bestStart < 0) continue;
    for (let j = bestStart; j < bestStart + k; j++) owner.set(r + j, a);
    const last = bestStart + k - 1;
    console.log(k === 1 ? `${a}: ${r}${bestStart}` : `${a}: ${r}${bestStart}-${r}${last}`);
    done = true;
    break;
  }
  if (!done) console.log(`${a}: no room`);
}

for (const r of rows) {
  let line = r + " ";
  for (let s = 1; s <= 10; s++) line += broken.has(r + s) ? "x" : owner.has(r + s) ? "#" : ".";
  console.log(line);
}
