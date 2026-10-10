const LIMIT = 2000000;
const isPrime: boolean[] = new Array(LIMIT).fill(true);
isPrime[0] = false;
isPrime[1] = false;
for (let i = 2; i * i < LIMIT; i++) {
  if (isPrime[i]) {
    for (let j = i * i; j < LIMIT; j += i) {
      isPrime[j] = false;
    }
  }
}
let count = 0;
let total = 0;
let largest = 0;
let twins = 0;
for (let n = 2; n < LIMIT; n++) {
  if (isPrime[n]) {
    count++;
    total += n;
    largest = n;
    if (n + 2 < LIMIT && isPrime[n + 2]) {
      twins++;
    }
  }
}
console.log(count);
console.log(total);
console.log(largest);
console.log(twins);
