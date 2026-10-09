const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

const parseMoney = (text: string): number => {
  const m = /^([0-9]+)(?:\.([0-9]{1,2}))?$/.exec(text);
  if (m === null) return -1;
  return parseInt(m[1], 10) * 100 + parseInt((m[2] || "0").padEnd(2, "0"), 10);
};
const money = (cents: number): string => `${Math.floor(cents / 100)}.${String(cents % 100).padStart(2, "0")}`;

let subtotal = 0, rate = 0;
lines.forEach((line: string, i: number) => {
  const number = i + 1;
  if (line.replace(/^ +| +$/g, "") === "") return;
  if (line.startsWith("tax=")) {
    const r = parseMoney(line.slice(4));
    if (r < 0) console.log(`line ${number}: invalid`);
    else rate = r;
    return;
  }
  const parts = line.split(";");
  if (parts.length === 3 && parts[0] !== "" && /^[0-9]{1,4}$/.test(parts[1]) && parseInt(parts[1], 10) >= 1) {
    const price = parseMoney(parts[2]);
    if (price >= 0) {
      const qty = parseInt(parts[1], 10);
      subtotal += qty * price;
      console.log(`${parts[0]}: ${qty} x ${money(price)} = ${money(qty * price)}`);
      return;
    }
  }
  console.log(`line ${number}: invalid`);
});
const tax = Math.floor((subtotal * rate + 5000) / 10000);
console.log(`subtotal=${money(subtotal)}`);
console.log(`tax=${money(tax)}`);
console.log(`total=${money(subtotal + tax)}`);
