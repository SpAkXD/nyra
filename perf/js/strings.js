// string building, split, join, char scans (reference for strings.nyra)
let s = "";
for (let i = 0; i < 2000000; i++) {
  s += i;
  s += ",";
}
console.log(String(s.length));
let sevens = 0;
for (let k = 0; k < s.length; k++) if (s.charCodeAt(k) === 55) sevens++;
console.log(String(sevens));
const parts = s.split(",");
let total = 0;
for (const p of parts) if (p.length > 0) total += parseInt(p, 10);
console.log(String(total));
const lines = [];
for (let i = 0; i < 500000; i++) lines.push(`item ${i}: ${i * 3} of ${i % 7}`);
const text = lines.join("\n");
console.log(String(text.length));
