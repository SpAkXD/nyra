function gcd(a: number, b: number): number {
  while (b !== 0) {
    [a, b] = [b, a % b];
  }
  return a;
}

const pairs: [number, number][] = [[48, 18], [1071, 462], [17, 5], [1000000, 250000], [270, 192]];
for (const [a, b] of pairs) {
  console.log(gcd(a, b));
}
