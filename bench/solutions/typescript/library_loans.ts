const events = "1 borrow ann B1; 1 borrow bob B1; 2 hold bob B1; 2 hold cat B1; 3 hold bob B1; 3 hold ann B1; 4 hold cat B2; 5 borrow ann B2; 6 borrow ann B3; 6 borrow bob B3; 15 return ann B1; 16 borrow ann B1; 17 borrow bob B1; 18 hold ann B1; 20 return ann B2; 45 return bob B3; 46 borrow bob B2; 47 pay bob 250; 47 borrow bob B2; 48 return cat B1; 50 return bob B1; 51 hold cat B1; 52 borrow ann B1; 53 borrow cat B1; 54 pay ann 100; 55 hold ann B1; 56 return cat B1; 57 borrow ann B1; 58 pay bob 700".split("; ");

const books = ["B1", "B2", "B3"];
const members = ["ann", "bob", "cat"];
const balance = new Map<string, number>(members.map((m) => [m, 0]));
const loanedTo = new Map<string, string>();
const due = new Map<string, number>();
const heldFor = new Map<string, string>();
const waiting = new Map<string, string[]>(books.map((b) => [b, []]));

const money = (c: number) => `${Math.floor(c / 100)}.${String(c % 100).padStart(2, "0")}`;
const loansOf = (m: string) => books.filter((b) => loanedTo.get(b) === m);

for (const ev of events) {
  const [dayText, kind, m, arg] = ev.split(" ");
  const day = Number(dayText);
  if (kind === "borrow") {
    const b = arg;
    if (balance.get(m)! >= 300) console.log(`${m} blocked (owes ${money(balance.get(m)!)})`);
    else if (loansOf(m).length >= 2) console.log(`${m} at limit`);
    else if (loanedTo.has(b)) console.log(`${b} on loan until day ${due.get(b)}`);
    else if (heldFor.has(b) && heldFor.get(b) !== m) console.log(`${b} held for ${heldFor.get(b)}`);
    else {
      loanedTo.set(b, m);
      due.set(b, day + 14);
      heldFor.delete(b);
      console.log(`${m} borrowed ${b}, due day ${day + 14}`);
    }
  } else if (kind === "return") {
    const b = arg;
    if (loanedTo.get(b) !== m) {
      console.log(`${m} does not have ${b}`);
      continue;
    }
    loanedTo.delete(b);
    const d = due.get(b)!;
    if (day > d) {
      const late = day - d;
      const fee = Math.min(25 * late, 500);
      balance.set(m, balance.get(m)! + fee);
      console.log(`${m} returned ${b}, ${late} days late, fee ${money(fee)}`);
    } else console.log(`${m} returned ${b}`);
    const w = waiting.get(b)!;
    if (w.length > 0) {
      const h = w.shift()!;
      heldFor.set(b, h);
      console.log(`${b} held for ${h}`);
    }
  } else if (kind === "hold") {
    const b = arg;
    const w = waiting.get(b)!;
    if (loanedTo.get(b) === m) console.log(`${m} already has ${b}`);
    else if (!loanedTo.has(b) && !heldFor.has(b)) console.log(`${b} is available`);
    else if (w.includes(m) || heldFor.get(b) === m) console.log(`${m} already waiting for ${b}`);
    else {
      w.push(m);
      console.log(`${m} waiting for ${b}, position ${w.length}`);
    }
  } else {
    const x = Number(arg);
    const bal = balance.get(m)!;
    if (x > bal) {
      balance.set(m, 0);
      console.log(`${m} paid ${money(bal)}, change ${money(x - bal)}`);
    } else {
      balance.set(m, bal - x);
      console.log(`${m} paid ${money(x)}, owes ${money(bal - x)}`);
    }
  }
}

for (const m of members) {
  const l = loansOf(m);
  console.log(`${m} owes ${money(balance.get(m)!)}, has ${l.length ? l.join(", ") : "none"}`);
}
