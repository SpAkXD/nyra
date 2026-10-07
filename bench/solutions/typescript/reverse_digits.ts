function reverse(n: number): number {
  let result = 0;
  while (n > 0) {
    result = result * 10 + (n % 10);
    n = Math.floor(n / 10);
  }
  return result;
}

for (const n of [12345, 1200, 907, 86420]) {
  console.log(reverse(n));
}
