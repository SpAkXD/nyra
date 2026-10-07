const limit = 60;
const isPrime: boolean[] = new Array(limit).fill(true);
isPrime[0] = false;
isPrime[1] = false;
for (let i = 2; i < limit; i++) {
  if (isPrime[i]) {
    for (let multiple = i * i; multiple < limit; multiple += i) {
      isPrime[multiple] = false;
    }
  }
}
for (let i = 0; i < limit; i++) {
  if (isPrime[i]) console.log(i);
}
