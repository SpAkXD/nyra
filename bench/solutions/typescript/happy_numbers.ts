function next(n: number): number {
  let sum = 0;
  for (let x = n; x > 0; x = Math.floor(x / 10)) {
    sum += (x % 10) * (x % 10);
  }
  return sum;
}

// Floyd's cycle detection: a slow and a fast walker meet on the cycle, or the fast one reaches 1
function isHappy(n: number): boolean {
  let slow = n;
  let fast = next(n);
  while (fast !== 1 && slow !== fast) {
    slow = next(slow);
    fast = next(next(fast));
  }
  return fast === 1;
}

const happy: number[] = [];
for (let n = 1; happy.length < 10; n++) {
  if (isHappy(n)) happy.push(n);
}
for (const n of happy) {
  console.log(n);
}
