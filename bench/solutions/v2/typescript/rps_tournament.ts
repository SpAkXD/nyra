const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

let part1 = 0, part2 = 0;
lines.forEach((line: string, i: number) => {
  if (line === "") return;
  if (line.length !== 3 || !"ABC".includes(line[0]) || line[1] !== " " || !"XYZ".includes(line[2])) {
    console.log(`line ${i + 1}: invalid`);
    return;
  }
  const opp = "ABC".indexOf(line[0]);
  const second = "XYZ".indexOf(line[2]);
  const diff = (second - opp + 3) % 3;
  part1 += second + 1 + [3, 6, 0][diff];
  const mine = (opp + second + 2) % 3;
  part2 += mine + 1 + 3 * second;
});
console.log(`part1=${part1} part2=${part2}`);
