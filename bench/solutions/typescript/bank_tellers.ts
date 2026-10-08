const data = "ann 0 5 regular; bob 0 3 regular; cy 1 4 vip; dee 2 6 regular; eli 2 2 vip; fay 3 4 regular; gus 5 1 regular; hal 18 9 regular; ivy 19 3 vip; jon 20 4 regular; kim 20 5 vip; lou 22 4 regular; max 27 1 regular; ned 30 2 vip";

type Customer = { name: string; arrival: number; service: number; vip: boolean; wait: number };
const customers: Customer[] = data.split("; ").map((s) => {
  const [name, a, d, kind] = s.split(" ");
  return { name, arrival: Number(a), service: Number(d), vip: kind === "vip", wait: -1 };
});

const tellerEnd = [-1, -1, -1]; // minute when teller becomes free; -1 = free
const busy = [false, false, false];
const line: Customer[] = [];
let served = 0;
for (let t = 0; served < customers.length; t++) {
  for (let k = 0; k < 3; k++) if (busy[k] && tellerEnd[k] === t) busy[k] = false;
  for (const c of customers) {
    if (c.arrival !== t) continue;
    if (c.vip) {
      let idx = 0;
      while (idx < line.length && line[idx].vip) idx++;
      line.splice(idx, 0, c);
    } else line.push(c);
  }
  while (line.length > 0) {
    let teller = -1;
    for (let k = 0; k < 3; k++) {
      if (busy[k]) continue;
      if (k === 2 && t >= 20 && t <= 29) continue;
      teller = k;
      break;
    }
    if (teller < 0) break;
    const c = line.shift()!;
    busy[teller] = true;
    tellerEnd[teller] = t + c.service;
    c.wait = t - c.arrival;
    served++;
    console.log(`${t} ${c.name} teller ${teller + 1} wait ${c.wait} end ${t + c.service}`);
  }
}

let total = 0;
let best = customers[0];
for (const c of customers) {
  total += c.wait;
  if (c.wait > best.wait) best = c;
}
console.log(`total wait ${total}, longest wait ${best.wait} (${best.name})`);
