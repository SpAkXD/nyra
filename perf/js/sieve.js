// sieve of Eratosthenes to 10,000,000, ten rounds (reference for sieve.nyra)
function countPrimes(n) {
  const sieve = new Uint8Array(n + 1).fill(1);
  sieve[0] = sieve[1] = 0;
  for (let i = 2; i * i <= n; i++) {
    if (sieve[i]) for (let j = i * i; j <= n; j += i) sieve[j] = 0;
  }
  let count = 0;
  for (let k = 0; k <= n; k++) count += sieve[k];
  return count;
}
let total = 0;
for (let round = 0; round < 10; round++) total += countPrimes(10000000 - round);
console.log(String(total));
