function isPrime(n: number): boolean {
  if (n < 2) return false;
  for (let d = 2; d * d <= n; d++) {
    if (n % d === 0) return false;
  }
  return true;
}

for (let n = 2; n <= 100; n++) {
  if (isPrime(n)) console.log(n);
}
