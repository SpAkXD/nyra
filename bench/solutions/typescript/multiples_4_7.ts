let sum = 0;
for (let n = 1; n < 1000; n++) {
  if (n % 4 === 0 || n % 7 === 0) sum += n;
}
console.log(sum);
