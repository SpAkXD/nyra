// recursion: fib(n) for n = 0 to 35, summed (reference for fib.nyra)
function fib(n) {
  return n < 2 ? n : fib(n - 1) + fib(n - 2);
}
let total = 0;
for (let n = 0; n < 36; n++) total += fib(n);
console.log(String(total));
