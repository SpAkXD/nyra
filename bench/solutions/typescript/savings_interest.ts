const months = [
  "deposit 1200.00; withdraw 1500.00; withdraw 197.50",
  "deposit 4000.50; withdraw 20.00",
  "withdraw 3978.75; deposit 6000.00",
  "withdraw 7500.00; withdraw 1997.76",
  "withdraw 5011.26",
  "withdraw 2.00; deposit 0.50",
];
const cents = (s: string): bigint => {
  const [a, b] = s.split(".");
  return BigInt(a) * 100n + BigInt(b);
};
const money = (c: bigint): string => {
  const neg = c < 0n;
  const v = neg ? -c : c;
  return (neg ? "-" : "") + (v / 100n).toString() + "." + (v % 100n).toString().padStart(2, "0");
};
let bal = 0n;
let totI = 0n;
let totF = 0n;
months.forEach((line, idx) => {
  const M = idx + 1;
  let min = bal;
  for (const tx of line.split("; ")) {
    const [op, x] = tx.split(" ");
    const v = cents(x);
    if (op === "deposit") bal += v;
    else if (bal - v < 0n) console.log(`month ${M}: withdraw ${x} rejected`);
    else bal -= v;
    if (bal < min) min = bal;
  }
  // yearly interest in units of cents * 1000 (rates in tenths of a percent)
  const t1 = min < 100000n ? min : 100000n;
  const t2 = min > 100000n ? (min < 500000n ? min : 500000n) - 100000n : 0n;
  const t3 = min > 500000n ? min - 500000n : 0n;
  const yearly = t1 * 15n + t2 * 24n + t3 * 30n; // cents * 1000
  const den = 12000n;
  let q = yearly / den;
  const r = yearly % den;
  if (r * 2n > den || (r * 2n === den && q % 2n === 1n)) q += 1n;
  const interest = q;
  bal += interest;
  let fee = 0n;
  if (min < 50000n) fee = bal < 300n ? bal : 300n;
  bal -= fee;
  totI += interest;
  totF += fee;
  console.log(`month ${M}: min ${money(min)} interest ${money(interest)} fee ${money(fee)} balance ${money(bal)}`);
});
console.log(`total interest ${money(totI)} fees ${money(totF)}`);
