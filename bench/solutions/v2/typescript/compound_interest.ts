const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

const [principal, rate, years, periods] = input.split(/\s+/).filter((t: string) => t !== "").map(parseFloat);
let balance = principal;
for (let year = 1; year <= years; year++) {
  for (let p = 0; p < periods; p++) balance = balance * (1 + rate / 100 / periods);
  console.log(`Year ${year}: ${balance.toFixed(2)}`);
}
console.log(`interest earned: ${(balance - principal).toFixed(2)}`);
