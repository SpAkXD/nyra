function fib(n: number): number {
  if (n <= 2) return 1;
  return fib(n - 1) + fib(n - 2);
}

for (const n of [10, 20, 25]) {
  console.log(fib(n));
}
