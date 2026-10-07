function gcd(a: number, b: number): number {
  while (b !== 0) {
    [a, b] = [b, a % b];
  }
  return a;
}

let result = 1;
for (let n = 2; n <= 16; n++) {
  result = (result / gcd(result, n)) * n;
}
console.log(result);
