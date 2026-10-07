let count = 0;
for (let a = 1; a <= 50; a++) {
  for (let b = a + 1; b <= 50; b++) {
    if ((a + b) % 5 === 0) count++;
  }
}
console.log(count);
