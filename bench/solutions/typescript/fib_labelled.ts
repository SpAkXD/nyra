let a = 0;
let b = 1;
for (let n = 0; n < 10; n++) {
  console.log(`fib(${n}) = ${a}`);
  [a, b] = [b, a + b];
}
