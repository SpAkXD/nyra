// Euclid's formula: every primitive triple is (m*m - n*n, 2*m*n, m*m + n*n) for coprime m > n of different parity;
// the others are its multiples.
function gcd(a: number, b: number): number {
  return b === 0 ? a : gcd(b, a % b);
}

const triples: [number, number, number][] = [];
for (let m = 2; m * m <= 50; m++) {
  for (let n = 1; n < m; n++) {
    if ((m - n) % 2 === 0 || gcd(m, n) !== 1) continue;
    const x = m * m - n * n;
    const y = 2 * m * n;
    const z = m * m + n * n;
    for (let k = 1; k * z <= 50; k++) {
      triples.push([k * Math.min(x, y), k * Math.max(x, y), k * z]);
    }
  }
}
triples.sort((p, q) => p[2] - q[2] || p[0] - q[0]);
for (const [a, b, c] of triples) {
  console.log(`${a} ${b} ${c}`);
}
