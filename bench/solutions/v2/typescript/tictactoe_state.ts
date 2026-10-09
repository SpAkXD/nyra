const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

const LINES = [[0, 1, 2], [3, 4, 5], [6, 7, 8], [0, 3, 6], [1, 4, 7], [2, 5, 8], [0, 4, 8], [2, 4, 6]];
const count = (s: string, c: string): number => s.split("").filter((x: string) => x === c).length;
const wins = (s: string, mark: string): boolean => LINES.some((l: number[]) => l.every((i: number) => s[i] === mark));

function state(s: string): string {
  if (s.length !== 9 || !/^[XO.]+$/.test(s)) return "invalid";
  const x = count(s, "X"), o = count(s, "O");
  if (!(x === o || x === o + 1)) return "invalid";
  const xw = wins(s, "X"), ow = wins(s, "O");
  if (xw && ow) return "invalid";
  if (xw) return x === o + 1 ? "X wins" : "invalid";
  if (ow) return x === o ? "O wins" : "invalid";
  return s.includes(".") ? "in progress" : "draw";
}

for (const line of lines) if (line !== "") console.log(state(line));
