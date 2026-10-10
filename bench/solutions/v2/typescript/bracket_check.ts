const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

const CLOSERS: { [c: string]: string } = { ")": "(", "]": "[", "}": "{" };
for (const line of lines) {
  const stack: Array<[string, number]> = [];
  let bad = 0;
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if ("([{".includes(c)) {
      stack.push([c, i + 1]);
    } else if (")]}".includes(c)) {
      if (stack.length === 0 || stack[stack.length - 1][0] !== CLOSERS[c]) {
        bad = i + 1;
        break;
      }
      stack.pop();
    }
  }
  if (bad === 0 && stack.length > 0) bad = stack[stack.length - 1][1];
  console.log(bad === 0 ? "OK" : `ERROR at ${bad}`);
}
