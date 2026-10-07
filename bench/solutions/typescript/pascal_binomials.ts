function binom(n: number, k: number): number {
  if (k === 0 || k === n) return 1;
  return binom(n - 1, k - 1) + binom(n - 1, k);
}

const cases: [number, number][] = [[5, 2], [10, 5], [20, 10]];
for (const [n, k] of cases) {
  console.log(binom(n, k));
}
