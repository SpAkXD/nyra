const WD = ["Ari", "Bel", "Cor", "Dun", "Eld", "Fen"];
const isLeap = (y: number) => (y % 6 === 0 && y % 90 !== 0) || y % 360 === 0;
const monthLen = (y: number, m: number) => (m < 10 ? 36 : isLeap(y) ? 42 : 41);
const yearLen = (y: number) => (isLeap(y) ? 366 : 365);
type D = [number, number, number];
function parse(s: string): D | null {
  const p = s.split("-").map(Number);
  const [y, m, d] = p;
  if (y < 1 || m < 1 || m > 10 || d < 1 || d > monthLen(y, m)) return null;
  return [y, m, d];
}
function toNum(x: D): number {
  let n = 0;
  for (let y = 1; y < x[0]; y++) n += yearLen(y);
  for (let m = 1; m < x[1]; m++) n += monthLen(x[0], m);
  return n + x[2] - 1;
}
function fromNum(n: number): D {
  let y = 1;
  while (n >= yearLen(y)) { n -= yearLen(y); y++; }
  let m = 1;
  while (n >= monthLen(y, m)) { n -= monthLen(y, m); m++; }
  return [y, m, n + 1];
}
const pad = (v: number, w: number) => String(v).padStart(w, "0");
const fmt = (n: number) => {
  const [y, m, d] = fromNum(n);
  return `${pad(y, 4)}-${pad(m, 2)}-${pad(d, 2)} ${WD[n % 6]}`;
};
const queries = "ADD 0001-01-01 0; ADD 0359-10-40 3; ADD 0360-10-41 1; ADD 0450-03-17 -1000; DIFF 0090-10-41 0091-01-01; DIFF 0721-05-30 0001-01-01; ADD 0539-10-42 1; NTH 3 Dun 0540-07; NTH 7 Fen 0012-10; NTH 7 Ari 0013-10; DIFF 0180-10-42 0181-01-01; ADD 0002-01-01 -365; NTH 7 Bel 0013-10; ADD 0005-11-01 1; ADD 0359-01-01 365";
for (const q of queries.split("; ")) {
  const p = q.split(" ");
  if (p[0] === "ADD") {
    const d = parse(p[1]);
    console.log(d ? fmt(toNum(d) + Number(p[2])) : "invalid date");
  } else if (p[0] === "DIFF") {
    const a = parse(p[1]);
    const b = parse(p[2]);
    console.log(a && b ? String(toNum(b) - toNum(a)) : "invalid date");
  } else {
    const k = Number(p[1]);
    const w = WD.indexOf(p[2]);
    const [y, m] = p[3].split("-").map(Number);
    if (y < 1 || m < 1 || m > 10) { console.log("invalid date"); continue; }
    const start = toNum([y, m, 1]);
    let count = 0;
    let ans = "none";
    for (let d = 0; d < monthLen(y, m); d++) {
      if ((start + d) % 6 === w) {
        count++;
        if (count === k) { ans = fmt(start + d); break; }
      }
    }
    console.log(ans);
  }
}
