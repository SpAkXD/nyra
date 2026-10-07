function isPrime(n: number): boolean {
  if (n < 2) return false;
  for (let d = 2; d * d <= n; d++) {
    if (n % d === 0) return false;
  }
  return true;
}

for (const n of [1, 2, 3, 4, 17, 25, 97, 100, 7919]) {
  console.log(isPrime(n) ? "true" : "false");
}
