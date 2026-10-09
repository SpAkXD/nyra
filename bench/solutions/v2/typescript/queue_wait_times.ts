const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

const customers: Array<[number, number]> = [];
for (const line of lines) {
  const parts = line.split(" ").filter((t: string) => t !== "");
  if (parts.length === 2) customers.push([parseInt(parts[0], 10), parseInt(parts[1], 10)]);
}
console.log(`customers=${customers.length}`);
if (customers.length > 0) {
  let freeAt = 0, busy = 0, maxWait = 0, totalWait = 0;
  for (const [arrival, service] of customers) {
    const start = Math.max(arrival, freeAt);
    const wait = start - arrival;
    totalWait += wait;
    maxWait = Math.max(maxWait, wait);
    freeAt = start + service;
    busy += service;
  }
  const span = freeAt - customers[0][0];
  console.log(`average wait=${(totalWait / customers.length).toFixed(2)} max wait=${maxWait}`);
  console.log(`busy=${((busy * 100) / span).toFixed(1)}%`);
}
