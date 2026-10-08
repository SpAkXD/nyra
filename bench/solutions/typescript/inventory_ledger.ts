type Item = { h: number; r: number };
const items = new Map<string, Item>();
const cmds = "receive bolt 12; reserve bolt 9; reserve nut 1; receive nut 4; reserve bolt 4; receive bolt 10; ship bolt 11; release bolt 1; reserve bolt 7; count bolt 6; count bolt 15; release bolt 7; ship nut 4; ship nut 1; receive washer 30; reserve washer 25; ship washer 1; count washer 26; count nut 0; ship bolt 15; receive bolt 5; release gear 2";
for (const c of cmds.split("; ")) {
  const [op, name, ns] = c.split(" ");
  const n = Number(ns);
  let it = items.get(name);
  if (op !== "receive" && !it) {
    console.log(`${name} unknown`);
    continue;
  }
  const before = it ? it.h - it.r : 0;
  let ok = true;
  if (op === "receive") {
    if (!it) { it = { h: 0, r: 0 }; items.set(name, it); }
    it.h += n;
    console.log(`${name} on-hand ${it.h} available ${it.h - it.r}`);
  } else if (op === "reserve") {
    const a = it!.h - it!.r;
    if (n > a) { ok = false; console.log(`${name} reserve rejected (available ${a})`); }
    else { it!.r += n; console.log(`${name} reserved ${it!.r} available ${it!.h - it!.r}`); }
  } else if (op === "release") {
    if (n > it!.r) { ok = false; console.log(`${name} release rejected (reserved ${it!.r})`); }
    else { it!.r -= n; console.log(`${name} reserved ${it!.r} available ${it!.h - it!.r}`); }
  } else if (op === "ship") {
    if (n > it!.h) { ok = false; console.log(`${name} ship rejected (on-hand ${it!.h})`); }
    else {
      it!.h -= n;
      it!.r = Math.max(0, it!.r - n);
      console.log(`${name} shipped ${n} on-hand ${it!.h} reserved ${it!.r}`);
    }
  } else if (op === "count") {
    if (n < it!.r) { ok = false; console.log(`${name} count rejected (reserved ${it!.r})`); }
    else {
      const d = n - it!.h;
      it!.h = n;
      console.log(`${name} adjusted by ${d > 0 ? "+" + d : String(d)}`);
    }
  }
  if (ok) {
    const after = it!.h - it!.r;
    if (after < 5 && before >= 5) console.log(`${name} low stock`);
  }
}
console.log("---");
for (const name of [...items.keys()].sort()) {
  const it = items.get(name)!;
  console.log(`${name}: on-hand ${it.h}, reserved ${it.r}, available ${it.h - it.r}`);
}
