const slots = new Map<string, { price: number; stock: number }>([
  ["A1", { price: 65, stock: 3 }],
  ["A2", { price: 100, stock: 1 }],
  ["B1", { price: 45, stock: 0 }],
  ["B2", { price: 120, stock: 2 }],
]);
const VALUES = [100, 25, 10, 5];
const store = new Map<number, number>([[100, 0], [25, 2], [10, 1], [5, 3]]);
let credit = 0;
function makeChange(amount: number): { taken: number[]; left: number } {
  const taken: number[] = [];
  let left = amount;
  for (const v of VALUES) {
    const k = Math.min(Math.floor(left / v), store.get(v)!);
    taken.push(k);
    left -= k * v;
  }
  return { taken, left };
}
function give(taken: number[]): string {
  const parts: string[] = [];
  VALUES.forEach((v, i) => {
    if (taken[i] > 0) {
      store.set(v, store.get(v)! - taken[i]);
      parts.push(`${v}x${taken[i]}`);
    }
  });
  return parts.length ? parts.join(" ") : "none";
}
const events = "insert 25; insert 25; insert 3; select A1; insert 25; select A1; insert 100; select B1; select C9; select A2; select A2; insert 100; insert 25; select B2; insert 100; select A1; insert 100; select A1; cancel; insert 10; insert 10; insert 10; insert 100; select A1; cancel; select B2; insert 50; cancel";
for (const e of events.split("; ")) {
  const [op, arg] = e.split(" ");
  if (op === "insert") {
    const n = Number(arg);
    if (!VALUES.includes(n)) { console.log(`rejected ${n}`); continue; }
    store.set(n, store.get(n)! + 1);
    credit += n;
    console.log(`credit ${credit}`);
  } else if (op === "select") {
    const s = slots.get(arg);
    if (!s) console.log(`no slot ${arg}`);
    else if (s.stock === 0) console.log(`${arg} sold out`);
    else if (credit < s.price) console.log(`${arg} costs ${s.price}, insert ${s.price - credit} more`);
    else {
      const ch = makeChange(credit - s.price);
      if (ch.left > 0) console.log(`${arg} exact change only`);
      else {
        s.stock -= 1;
        credit = 0;
        console.log(`${arg} vended, change ${give(ch.taken)}`);
      }
    }
  } else {
    const ch = makeChange(credit);
    credit = 0;
    const list = give(ch.taken);
    console.log(`returned ${list}${ch.left > 0 ? ` (owed ${ch.left})` : ""}`);
  }
}
console.log(`stock A1 ${slots.get("A1")!.stock} A2 ${slots.get("A2")!.stock} B1 ${slots.get("B1")!.stock} B2 ${slots.get("B2")!.stock}`);
console.log(`coins 100x${store.get(100)} 25x${store.get(25)} 10x${store.get(10)} 5x${store.get(5)}`);
