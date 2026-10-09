const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

function truncDiv(a: number, b: number): number {
  const q = Math.floor(Math.abs(a) / Math.abs(b));
  return (a < 0) !== (b < 0) ? -q : q;
}

for (const raw of lines) {
  const tokens = raw.split(" ").filter((t: string) => t !== "");
  if (tokens.length === 0) continue;
  const stack: number[] = [];
  let error = "";
  for (const token of tokens) {
    if (/^-?[0-9]+$/.test(token)) {
      stack.push(parseInt(token, 10));
    } else if (token === "+" || token === "-" || token === "*" || token === "/") {
      if (stack.length < 2) {
        error = "stack underflow";
        break;
      }
      const b = stack.pop() as number;
      const a = stack.pop() as number;
      if (token === "+") stack.push(a + b);
      else if (token === "-") stack.push(a - b);
      else if (token === "*") stack.push(a * b);
      else {
        if (b === 0) {
          error = "division by zero";
          break;
        }
        stack.push(truncDiv(a, b));
      }
    } else {
      error = `bad token ${token}`;
      break;
    }
  }
  if (error === "" && stack.length !== 1) error = "leftover values";
  console.log(error !== "" ? `error: ${error}` : String(stack[0]));
}
