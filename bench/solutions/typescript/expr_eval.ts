const exprs = [
  "1 + 2 * 3 - 4 / 2", "10 - 4 - 3", "2 ^ 3 ^ 2", "-2 ^ 2", "(-2) ^ 2", "2 * (3 + 4) ^ 2",
  "100 / 7 % 4", "- - 3", "8/3*3+8%3", "((15 % 4) ^ 2 - -3) * 2", "0 * (5 / (2 - 2))",
  "0^0 + 2^10 - 1000", "-(3 - 10) * -2 ^ 3", "7 - (2 - (3 - (4 - 5)))", "2 ^ (1 + 1) ^ 3",
  "17 % 5 ^ 2 / 3", "(1 + 2) * (3 % (4 - 4)) + 1", "-3 ^ 2 * -(1 + 1) ^ 2",
];
let toks: string[] = [];
let pos = 0;
let divZero = false;
function tokenize(s: string): string[] {
  const out: string[] = [];
  let i = 0;
  while (i < s.length) {
    const ch = s[i];
    if (ch === " ") { i++; continue; }
    if (ch >= "0" && ch <= "9") {
      let j = i;
      while (j < s.length && s[j] >= "0" && s[j] <= "9") j++;
      out.push(s.slice(i, j));
      i = j;
    } else { out.push(ch); i++; }
  }
  return out;
}
function peek(): string | undefined { return toks[pos]; }
function sum(): bigint {
  let v = product();
  while (peek() === "+" || peek() === "-") {
    const op = toks[pos++];
    const r = product();
    v = op === "+" ? v + r : v - r;
  }
  return v;
}
function product(): bigint {
  let v = unary();
  while (peek() === "*" || peek() === "/" || peek() === "%") {
    const op = toks[pos++];
    const r = unary();
    if (op === "*") v = v * r;
    else if (r === 0n) { divZero = true; v = 0n; }
    else v = op === "/" ? v / r : v % r;
  }
  return v;
}
function unary(): bigint {
  if (peek() === "-") { pos++; return -unary(); }
  return power();
}
function power(): bigint {
  const base = atom();
  if (peek() === "^") {
    pos++;
    const e = power();
    let r = 1n;
    for (let i = 0n; i < e; i++) r *= base;
    return r;
  }
  return base;
}
function atom(): bigint {
  const t = toks[pos++];
  if (t === "(") {
    const v = sum();
    pos++;
    return v;
  }
  return BigInt(t);
}
for (const e of exprs) {
  toks = tokenize(e);
  pos = 0;
  divZero = false;
  const v = sum();
  console.log(`${e} = ${divZero ? "division by zero" : v.toString()}`);
}
