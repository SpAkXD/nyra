type M = number[][];
const parse = (s: string): M => s.split(";").map((r) => r.trim().split(/\s+/).map(Number));
const A = parse("2 -1 0; 1 3 -2; 0 4 1");
const B = parse("1 2; -3 0; 5 -1");
const C = parse("2 -1; 1 3");
const D = parse("3 1 2 -1; 1 2 0 -2; 4 -1 6 -3; 5 0 2 1");
function mul(x: M, y: M): M {
  const r: M = [];
  for (let i = 0; i < x.length; i++) {
    r.push([]);
    for (let j = 0; j < y[0].length; j++) {
      let s = 0;
      for (let k = 0; k < y.length; k++) s += x[i][k] * y[k][j];
      r[i].push(s);
    }
  }
  return r;
}
function transpose(x: M): M {
  return x[0].map((_, j) => x.map((row) => row[j]));
}
function show(title: string, x: M): void {
  console.log(title);
  const w = x[0].map((_, j) => Math.max(...x.map((row) => String(row[j]).length)));
  for (const row of x) console.log(row.map((v, j) => String(v).padStart(w[j])).join("  "));
}
function det(x: M): number {
  const n = x.length;
  if (n === 1) return x[0][0];
  let s = 0;
  for (let j = 0; j < n; j++) {
    const minor = x.slice(1).map((row) => row.filter((_, k) => k !== j));
    s += (j % 2 === 0 ? 1 : -1) * x[0][j] * det(minor);
  }
  return s;
}
const AB = mul(A, B);
show("A*B", AB);
show("(A*B)^T", transpose(AB));
let c6 = C;
for (let i = 1; i < 6; i++) c6 = mul(c6, C);
show("C^6", c6);
const AA = mul(A, A);
show("A*A-3A+2I", AA.map((row, i) => row.map((v, j) => v - 3 * A[i][j] + (i === j ? 2 : 0))));
console.log(`det(D) = ${det(D)}`);
