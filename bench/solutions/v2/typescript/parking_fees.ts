const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

const inside = new Map<string, number>();
let revenue = 0;
lines.forEach((line: string, i: number) => {
  const [when, kind, plate] = line.split(" ");
  const minutes = parseInt(when.slice(0, 2), 10) * 60 + parseInt(when.slice(3), 10);
  if (kind === "IN") {
    if (inside.has(plate)) console.log(`line ${i + 1}: already inside`);
    else inside.set(plate, minutes);
  } else {
    if (!inside.has(plate)) {
      console.log(`line ${i + 1}: not inside`);
    } else {
      const stay = minutes - (inside.get(plate) as number);
      inside.delete(plate);
      const fee = stay <= 15 ? 0 : Math.min(20, 2 * Math.floor((stay + 59) / 60));
      revenue += fee;
      console.log(`${plate} paid $${fee}`);
    }
  }
});
console.log(`revenue=$${revenue}`);
const left = Array.from(inside.keys()).sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
console.log("still inside: " + (left.length > 0 ? left.join(",") : "none"));
