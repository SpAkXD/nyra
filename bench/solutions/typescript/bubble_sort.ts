const numbers: number[] = [64, 34, 25, 12, 22, 11, 90];
const n = numbers.length;
for (let i = 0; i < n; i++) {
  for (let j = 0; j < n - 1 - i; j++) {
    if (numbers[j] > numbers[j + 1]) {
      const tmp = numbers[j];
      numbers[j] = numbers[j + 1];
      numbers[j + 1] = tmp;
    }
  }
}
for (const value of numbers) {
  console.log(value);
}
