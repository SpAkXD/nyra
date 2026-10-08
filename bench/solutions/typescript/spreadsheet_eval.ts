const cells: [string, string][] = [
  ["A1", "10"], ["B1", "=A1*2"], ["C1", "=B1+A2"], ["D1", "=SUM(A1:C1)"],
  ["A2", "3"], ["B2", "=A2-B1/4"], ["D2", "=MAX(A1:C2)*(C2+2)"],
  ["A3", "=B3+1"], ["B3", "=C3"], ["C3", "=A3*0"], ["D3", "=-C3"],
  ["A4", "=10/(A2-3)"], ["B4", "=A4+1"], ["C4", "=MAX(A1:B2)+D4"], ["D4", "=E1+1"],
  ["A5", "=SUM(A1:D2)-(A1+B1)*-2"], ["B5", "=B1/0+D4"], ["C5", "=SUM(C4:C5)"], ["D5", "=D4*0+B4"],
];

type Node =
  | { k: "num"; v: number }
  | { k: "ref"; name: string }
  | { k: "fn"; f: string; a: string; b: string }
  | { k: "neg"; e: Node }
  | { k: "bin"; op: string; l: Node; r: Node };

function parse(src: string): Node {
  const toks = src.match(/\d+|[A-Z]+\d+|[A-Z]+|[-+*/():]/g)!;
  let i = 0;
  const peek = () => toks[i];
  const next = () => toks[i++];
  function expr(): Node {
    let l = term();
    while (peek() === "+" || peek() === "-") {
      const op = next();
      l = { k: "bin", op, l, r: term() };
    }
    return l;
  }
  function term(): Node {
    let l = factor();
    while (peek() === "*" || peek() === "/") {
      const op = next();
      l = { k: "bin", op, l, r: factor() };
    }
    return l;
  }
  function factor(): Node {
    const t = next();
    if (t === "-") return { k: "neg", e: factor() };
    if (t === "(") {
      const e = expr();
      next();
      return e;
    }
    if (/^\d+$/.test(t)) return { k: "num", v: Number(t) };
    if (/^[A-Z]+\d+$/.test(t)) return { k: "ref", name: t };
    next(); // (
    const a = next();
    next(); // :
    const b = next();
    next(); // )
    return { k: "fn", f: t, a, b };
  }
  return expr();
}

const valid = (n: string) => /^[A-D][1-5]$/.test(n);
function rect(a: string, b: string): string[] {
  const out: string[] = [];
  for (let r = Number(a[1]); r <= Number(b[1]); r++)
    for (let c = a.charCodeAt(0); c <= b.charCodeAt(0); c++) out.push(String.fromCharCode(c) + r);
  return out;
}

const formulas = new Map<string, Node>();
const values = new Map<string, number | string>();
for (const [n, s] of cells) {
  if (s.startsWith("=")) formulas.set(n, parse(s.slice(1)));
  else values.set(n, Number(s));
}

function deps(e: Node, out: string[]): string[] {
  if (e.k === "ref") {
    if (valid(e.name)) out.push(e.name);
  } else if (e.k === "fn") {
    if (valid(e.a) && valid(e.b)) out.push(...rect(e.a, e.b));
  } else if (e.k === "neg") deps(e.e, out);
  else if (e.k === "bin") {
    deps(e.l, out);
    deps(e.r, out);
  }
  return out;
}

const graph = new Map<string, string[]>();
for (const [n, f] of formulas) graph.set(n, deps(f, []));

function reach(start: string): Set<string> {
  const seen = new Set<string>();
  const stack = [...(graph.get(start) ?? [])];
  while (stack.length) {
    const x = stack.pop()!;
    if (seen.has(x)) continue;
    seen.add(x);
    stack.push(...(graph.get(x) ?? []));
  }
  return seen;
}

const onCycle = new Set([...graph.keys()].filter((n) => reach(n).has(n)));
for (const n of graph.keys()) {
  if (onCycle.has(n) || [...reach(n)].some((x) => onCycle.has(x))) values.set(n, "#CYCLE");
}

function cellValue(n: string): number | string {
  if (values.has(n)) return values.get(n)!;
  if (!formulas.has(n)) return 0;
  const v = evalNode(formulas.get(n)!);
  values.set(n, v);
  return v;
}

function evalNode(e: Node): number | string {
  switch (e.k) {
    case "num":
      return e.v;
    case "ref":
      return valid(e.name) ? cellValue(e.name) : "#REF";
    case "neg": {
      const v = evalNode(e.e);
      return typeof v === "string" ? v : -v;
    }
    case "fn": {
      if (!valid(e.a) || !valid(e.b)) return "#REF";
      const vs: number[] = [];
      for (const c of rect(e.a, e.b)) {
        const v = cellValue(c);
        if (typeof v === "string") return v;
        vs.push(v);
      }
      return e.f === "SUM" ? vs.reduce((s, x) => s + x, 0) : Math.max(...vs);
    }
    case "bin": {
      const l = evalNode(e.l);
      if (typeof l === "string") return l;
      const r = evalNode(e.r);
      if (typeof r === "string") return r;
      if (e.op === "+") return l + r;
      if (e.op === "-") return l - r;
      if (e.op === "*") return l * r;
      if (r === 0) return "#DIV0";
      return Math.trunc(l / r);
    }
  }
}

for (const [n] of cells) console.log(`${n} = ${cellValue(n)}`);
