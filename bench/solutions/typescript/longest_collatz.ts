function steps(n: number): number {
  let count = 0;
  while (n !== 1) {
    n = n % 2 === 0 ? n / 2 : 3 * n + 1;
    count++;
  }
  return count;
}

let best = 1;
let bestSteps = 0;
for (let start = 1; start < 10000; start++) {
  const s = steps(start);
  if (s > bestSteps) {
    best = start;
    bestSteps = s;
  }
}
console.log(best);
console.log(bestSteps);
