type Poly = Map<number, number>; // degree -> coefficient

function parseTerm(term: string, sign: number, p: Poly): void {
  if (term.startsWith("-")) {
    sign = -sign;
    term = term.slice(1);
  }
  let deg = 0;
  let coefText = term;
  const xi = term.indexOf("x");
  if (xi >= 0) {
    coefText = term.slice(0, xi);
    const rest = term.slice(xi + 1);
    deg = rest === "" ? 1 : Number(rest.slice(1));
  }
  const coef = coefText === "" ? 1 : Number(coefText);
  p.set(deg, (p.get(deg) ?? 0) + sign * coef);
}

function parse(text: string): Poly {
  const p: Poly = new Map();
  const tokens = text.split(" ");
  parseTerm(tokens[0], 1, p);
  for (let i = 1; i < tokens.length; i += 2) parseTerm(tokens[i + 1], tokens[i] === "-" ? -1 : 1, p);
  return norm(p);
}

function norm(p: Poly): Poly {
  const r: Poly = new Map();
  for (const [d, c] of p) if (c !== 0) r.set(d, c);
  return r;
}

function add(a: Poly, b: Poly, s = 1): Poly {
  const r: Poly = new Map(a);
  for (const [d, c] of b) r.set(d, (r.get(d) ?? 0) + s * c);
  return norm(r);
}

function mul(a: Poly, b: Poly): Poly {
  const r: Poly = new Map();
  for (const [d1, c1] of a) for (const [d2, c2] of b) r.set(d1 + d2, (r.get(d1 + d2) ?? 0) + c1 * c2);
  return norm(r);
}

function deriv(a: Poly): Poly {
  const r: Poly = new Map();
  for (const [d, c] of a) if (d > 0) r.set(d - 1, c * d);
  return norm(r);
}

function compose(a: Poly, b: Poly): Poly {
  let r: Poly = new Map();
  for (const [d, c] of a) {
    let pw: Poly = new Map([[0, 1]]);
    for (let i = 0; i < d; i++) pw = mul(pw, b);
    r = add(r, mul(pw, new Map([[0, c]])));
  }
  return r;
}

function evalAt(a: Poly, x: number): number {
  let s = 0;
  for (const [d, c] of a) s += c * x ** d;
  return s;
}

function show(a: Poly): string {
  const degs = [...a.keys()].sort((x, y) => y - x);
  if (degs.length === 0) return "0";
  let out = "";
  degs.forEach((d, i) => {
    const c = a.get(d)!;
    const abs = Math.abs(c);
    let body = d === 0 ? String(abs) : (abs === 1 ? "" : String(abs)) + (d === 1 ? "x" : `x^${d}`);
    if (i === 0) out += (c < 0 ? "-" : "") + body;
    else out += (c < 0 ? " - " : " + ") + body;
  });
  return out;
}

const P = parse("2x + 3 - x + x^2 - 4");
const Q = parse("-x^3 + 2x - 1 + x^3 + x^2 + x^10 - x^10");
const R = parse("5 - 3x^2");
const PQ = mul(P, Q);
const QR = compose(Q, R);
console.log(`P = ${show(P)}`);
console.log(`Q = ${show(Q)}`);
console.log(`R = ${show(R)}`);
console.log(`P + Q = ${show(add(P, Q))}`);
console.log(`P - R = ${show(add(P, R, -1))}`);
console.log(`P * Q = ${show(PQ)}`);
console.log(`(P * Q)' = ${show(deriv(PQ))}`);
console.log(`Q(R) = ${show(QR)}`);
console.log(`R(P) - R = ${show(add(compose(R, P), R, -1))}`);
console.log(`P - P = ${show(add(P, P, -1))}`);
console.log(`P(-3) = ${evalAt(P, -3)}`);
console.log(`Q(R)(2) = ${evalAt(QR, 2)}`);
