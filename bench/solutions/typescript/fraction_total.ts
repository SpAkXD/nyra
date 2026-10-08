const ops = "add 1/2; sub 3/4; mul -2/3; div 5/6; add 7/3; div 0/5; mul 4/-6; add 10/4; sub -1/12; mul 0/7; add -9/4; div -3/2; sub 5/3; add 2/3; div 1/-4".split("; ");

type Frac = { n: bigint; d: bigint };

function gcd(a: bigint, b: bigint): bigint {
  if (a < 0n) a = -a;
  if (b < 0n) b = -b;
  while (b !== 0n) [a, b] = [b, a % b];
  return a;
}

function make(n: bigint, d: bigint): Frac {
  if (d < 0n) {
    n = -n;
    d = -d;
  }
  const g = gcd(n, d);
  return g === 0n ? { n: 0n, d: 1n } : { n: n / g, d: d / g };
}

function show(f: Frac): string {
  if (f.d === 1n) return String(f.n);
  const neg = f.n < 0n;
  const a = neg ? -f.n : f.n;
  const s = neg ? "-" : "";
  if (a < f.d) return `${s}${a}/${f.d}`;
  return `${s}${a / f.d} ${a % f.d}/${f.d}`;
}

const greater = (a: Frac, b: Frac) => a.n * b.d > b.n * a.d;

let total: Frac = { n: 0n, d: 1n };
let best: Frac | null = null;
let bestK = 0;
ops.forEach((op, i) => {
  const [word, fr] = op.split(" ");
  const [a, b] = fr.split("/").map((x) => BigInt(x));
  const f = make(a, b);
  if (word === "div" && f.n === 0n) {
    console.log(`${op}: cannot divide by zero, total ${show(total)}`);
    return;
  }
  if (word === "add") total = make(total.n * f.d + f.n * total.d, total.d * f.d);
  else if (word === "sub") total = make(total.n * f.d - f.n * total.d, total.d * f.d);
  else if (word === "mul") total = make(total.n * f.n, total.d * f.d);
  else total = make(total.n * f.d, total.d * f.n);
  console.log(`${op}: total ${show(total)}`);
  if (best === null || greater(total, best)) {
    best = total;
    bestK = i + 1;
  }
});
console.log(`largest total ${show(best!)} after operation ${bestK}`);
