function digitSum(n: number): number {
  let total = 0;
  while (n > 0) {
    total += n % 10;
    n = Math.floor(n / 10);
  }
  return total;
}

for (const n of [12345, 9999, 100000, 987654321]) {
  console.log(digitSum(n));
}
