const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

function score(tokens: string[]): number | null {
  if (!tokens.every((t: string) => /^(0|[1-9][0-9]?)$/.test(t))) return null;
  const r = tokens.map((t: string) => parseInt(t, 10));
  if (r.some((v: number) => v > 10)) return null;
  let total = 0, i = 0;
  for (let frame = 0; frame < 9; frame++) {
    if (i >= r.length) return null;
    if (r[i] === 10) {
      if (i + 2 >= r.length) return null;
      total += 10 + r[i + 1] + r[i + 2];
      i += 1;
    } else {
      if (i + 1 >= r.length || r[i] + r[i + 1] > 10) return null;
      total += r[i] + r[i + 1];
      if (r[i] + r[i + 1] === 10) {
        if (i + 2 >= r.length) return null;
        total += r[i + 2];
      }
      i += 2;
    }
  }
  const rest = r.slice(i);
  if (rest.length < 2) return null;
  const a = rest[0], b = rest[1];
  if (a === 10) {
    if (rest.length !== 3 || (b !== 10 && b + rest[2] > 10)) return null;
    return total + 10 + b + rest[2];
  }
  if (a + b > 10) return null;
  if (a + b === 10) return rest.length === 3 ? total + 10 + rest[2] : null;
  return rest.length === 2 ? total + a + b : null;
}

for (const line of lines) {
  if (line.replace(/^ +| +$/g, "") === "") continue;
  const result = score(line.split(" "));
  console.log(result === null ? "invalid" : String(result));
}
