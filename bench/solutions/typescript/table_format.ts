const data: [string, number, number, string][] = [
  ["bolts", 12000, 5, "bulk"],
  ["hex nuts", 350, 12, "galvanized"],
  ["washers", 7, 123456, "special order"],
  ["anchor", 1, 99, ""],
  ["rivets", 1500, 3, "aluminium"],
  ["brackets", 24, 1050, "left-hand only"],
];

const group = (n: number) => String(n).replace(/\B(?=(\d{3})+(?!\d))/g, ",");
const dollars = (c: number) => `${group(Math.floor(c / 100))}.${String(c % 100).padStart(2, "0")}`;
const note = (s: string) => (s.length > 10 ? s.slice(0, 9) + "~" : s);

const header = ["item", "qty", "price", "amount", "note"];
const align = ["L", "R", "R", "R", "C"];
const rows = data.map(([item, q, p, n]) => [item, group(q), dollars(p), dollars(q * p), note(n)]);
const totalQty = data.reduce((s, r) => s + r[1], 0);
const totalAmt = data.reduce((s, r) => s + r[1] * r[2], 0);
const totalRow = ["TOTAL", group(totalQty), "", dollars(totalAmt), ""];

const all = [header, ...rows, totalRow];
const widths = header.map((_, i) => Math.max(...all.map((r) => r[i].length)));

function cell(s: string, i: number): string {
  const pad = widths[i] - s.length;
  if (align[i] === "L") return s + " ".repeat(pad);
  if (align[i] === "R") return " ".repeat(pad) + s;
  const left = Math.floor(pad / 2);
  return " ".repeat(left) + s + " ".repeat(pad - left);
}

const line = (r: string[]) => "| " + r.map(cell).join(" | ") + " |";
const border = (ch: string) => "+" + widths.map((w) => ch.repeat(w + 2) + "+").join("");

console.log(border("-"));
console.log(line(header));
console.log(border("="));
for (const r of rows) console.log(line(r));
console.log(border("-"));
console.log(line(totalRow));
console.log(border("-"));
