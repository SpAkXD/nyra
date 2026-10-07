// smallest prime factor of every number up to the largest input, then repeated division by it
const LIMIT = 1000000;
const smallest = new Int32Array(LIMIT + 1);
for (let i = 2; i <= LIMIT; i++) {
  if (smallest[i] === 0) {
    for (let j = i; j <= LIMIT; j += i) {
      if (smallest[j] === 0) smallest[j] = i;
    }
  }
}

function factorization(n: number): string {
  const parts: string[] = [];
  let rest = n;
  while (rest > 1) {
    const p = smallest[rest];
    let exponent = 0;
    while (rest % p === 0) {
      rest /= p;
      exponent++;
    }
    parts.push(exponent > 1 ? `${p}^${exponent}` : `${p}`);
  }
  return `${n} = ${parts.join(" * ")}`;
}

for (const n of [360, 97, 1001, 65536, 999999]) {
  console.log(factorization(n));
}
