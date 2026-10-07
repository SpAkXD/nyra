const numbers: number[] = [4, 7, 1, 9, 6, 5, 2, 8];
const target = 11;
for (let i = 0; i < numbers.length; i++) {
  for (let j = i + 1; j < numbers.length; j++) {
    if (numbers[i] + numbers[j] === target) {
      console.log(`${i} ${j}`);
    }
  }
}
