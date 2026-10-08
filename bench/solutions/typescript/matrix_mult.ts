const N = 120;
const a: number[][] = [];
const b: number[][] = [];
for (let i = 0; i < N; i++) {
  a.push([]);
  b.push([]);
  for (let j = 0; j < N; j++) {
    a[i].push((i * 31 + j * 17 + 7) % 100);
    b[i].push((i * 13 + j * 29 + 3) % 100);
  }
}
const c: number[][] = [];
for (let i = 0; i < N; i++) {
  const row: number[] = [];
  for (let j = 0; j < N; j++) {
    let s = 0;
    for (let k = 0; k < N; k++) {
      s += a[i][k] * b[k][j];
    }
    row.push(s);
  }
  c.push(row);
}
let trace = 0;
let total = 0;
for (let i = 0; i < N; i++) {
  trace += c[i][i];
  for (let j = 0; j < N; j++) {
    total += c[i][j];
  }
}
console.log(trace);
console.log(total);
console.log(c[0][N - 1]);
console.log(c[N - 1][0]);
