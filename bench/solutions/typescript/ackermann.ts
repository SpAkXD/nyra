function ack(m: number, n: number): number {
  if (m === 0) return n + 1;
  if (n === 0) return ack(m - 1, 1);
  return ack(m - 1, ack(m, n - 1));
}

const cases: [number, number][] = [[0, 0], [1, 2], [2, 3], [3, 3], [3, 5]];
for (const [m, n] of cases) {
  console.log(ack(m, n));
}
