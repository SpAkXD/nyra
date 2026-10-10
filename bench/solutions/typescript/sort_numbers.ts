const N = 500000;
const M = 1000000007;
const xs: number[] = [];
let x = 12345;
for (let i = 0; i < N; i++) {
  x = (x * 48271) % 2147483647;
  xs.push(x);
}
xs.sort((p, q) => p - q);
let check = 0;
for (let i = 0; i < N; i++) {
  check = (check + (i + 1) * xs[i]) % M;
}
console.log(xs[0]);
console.log(xs[N - 1]);
console.log(xs[Math.floor(N / 2)]);
console.log(check);
