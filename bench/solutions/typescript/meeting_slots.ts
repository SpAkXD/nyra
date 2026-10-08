const toMin = (s: string) => Number(s.slice(0, 2)) * 60 + Number(s.slice(3));
const toStr = (m: number) => `${String(Math.floor(m / 60)).padStart(2, "0")}:${String(m % 60).padStart(2, "0")}`;

const people = ["ann", "bob", "cat", "dan"];
const busyText: Record<string, string> = {
  ann: "09:00-10:30, 12:00-13:00, 15:45-17:00",
  bob: "09:30-11:15, 13:00-14:00",
  cat: "11:00-12:15, 14:30-15:15",
  dan: "10:00-10:45, 13:30-16:00",
};
const busy = new Map<string, [number, number][]>();
for (const p of people) {
  busy.set(p, busyText[p].split(", ").map((s) => {
    const [a, b] = s.split("-");
    return [toMin(a), toMin(b)] as [number, number];
  }));
}

const DAY_START = toMin("09:00");
const DAY_END = toMin("17:00");
const requests = "ann+bob 60; bob+cat+dan 30; ann+cat 45; ann+bob+cat+dan 30; dan 120; cat+dan 15; ann+bob 45; bob 120".split("; ");

for (const req of requests) {
  const [names, durText] = req.split(" ");
  const who = names.split("+");
  const dur = Number(durText);
  let found = -1;
  for (let s = DAY_START; s + dur <= DAY_END; s += 15) {
    const e = s + dur;
    if (who.every((p) => busy.get(p)!.every(([bs, be]) => !(s < be && bs < e)))) {
      found = s;
      break;
    }
  }
  if (found < 0) console.log(`${names}: no slot`);
  else {
    for (const p of who) busy.get(p)!.push([found, found + dur]);
    console.log(`${names}: ${toStr(found)}-${toStr(found + dur)}`);
  }
}

for (const p of people) {
  const periods: string[] = [];
  let start = -1;
  for (let m = DAY_START; m <= DAY_END; m++) {
    const isFree = m < DAY_END && busy.get(p)!.every(([bs, be]) => !(m >= bs && m < be));
    if (isFree && start < 0) start = m;
    if (!isFree && start >= 0) {
      periods.push(`${toStr(start)}-${toStr(m)}`);
      start = -1;
    }
  }
  console.log(`${p} free: ${periods.length ? periods.join(", ") : "none"}`);
}
