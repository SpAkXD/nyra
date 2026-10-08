const cells = new Map<string, Map<number, number>>();
const get = (n: string, m: number) => cells.get(n)?.get(m) ?? 0;
const set = (n: string, m: number, v: number) => {
  if (!cells.has(n)) cells.set(n, new Map());
  cells.get(n)!.set(m, v);
};
const cmds = "add rent 1 900; add food 1 250; add food 2 -40; move rent 1 food 2 100; move food 2 rent 3 200; add fun 12 75; clear fun 12; clear fun 12; add rent 3 1300; move rent 3 rent 3 50; add zoo 5 0; add tax 4 -1300; move rent 3 tax 4 1300; add food 1 -250; add car 7 15; move car 7 car 8 15; add tax 10 -75; add food 10 12000; move food 10 rent 10 2500";
for (const c of cmds.split("; ")) {
  const p = c.split(" ");
  if (p[0] === "add") {
    const m = Number(p[2]);
    const v = get(p[1], m) + Number(p[3]);
    set(p[1], m, v);
    console.log(`${p[1]} ${m} = ${v}`);
  } else if (p[0] === "move") {
    const m1 = Number(p[2]);
    const m2 = Number(p[4]);
    const n = Number(p[5]);
    if (get(p[1], m1) < n) { console.log("move rejected"); continue; }
    set(p[1], m1, get(p[1], m1) - n);
    set(p[3], m2, get(p[3], m2) + n);
    console.log(`${p[1]} ${m1} = ${get(p[1], m1)}, ${p[3]} ${m2} = ${get(p[3], m2)}`);
  } else {
    const m = Number(p[2]);
    const v = get(p[1], m);
    if (v === 0) console.log(`${p[1]} ${m} already empty`);
    else { set(p[1], m, 0); console.log(`${p[1]} ${m} cleared (was ${v})`); }
  }
}
const accounts = [...cells.keys()].filter((a) => [...cells.get(a)!.values()].some((v) => v !== 0)).sort();
const months: number[] = [];
for (let m = 1; m <= 12; m++) if (accounts.some((a) => get(a, m) !== 0)) months.push(m);
const rows: string[][] = [["account", ...months.map(String), "total"]];
for (const a of accounts) {
  const vals = months.map((m) => get(a, m));
  let tot = 0;
  for (let m = 1; m <= 12; m++) tot += get(a, m);
  rows.push([a, ...vals.map((v) => (v === 0 ? "-" : String(v))), String(tot)]);
}
let grand = 0;
const colTot = months.map((m) => {
  let s = 0;
  for (const a of accounts) s += get(a, m);
  return s;
});
for (const a of accounts) for (let m = 1; m <= 12; m++) grand += get(a, m);
rows.push(["total", ...colTot.map(String), String(grand)]);
const w = rows[0].map((_, j) => Math.max(...rows.map((r) => r[j].length)));
for (const r of rows) {
  console.log(r.map((v, j) => (j === 0 ? v.padEnd(w[j]) : v.padStart(w[j]))).join("  "));
}
