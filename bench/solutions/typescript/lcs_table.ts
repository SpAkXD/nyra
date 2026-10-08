function sequence(seed: number, n: number): number[] {
  const out: number[] = [];
  let x = seed;
  for (let i = 0; i < n; i++) {
    x = (x * 75 + 74) % 65537;
    out.push(x % 4);
  }
  return out;
}

const N = 1000;
const a = sequence(1, N);
const b = sequence(2, N);
let prev: number[] = new Array(N + 1).fill(0);
for (let i = 1; i <= N; i++) {
  const cur: number[] = new Array(N + 1).fill(0);
  const ai = a[i - 1];
  for (let j = 1; j <= N; j++) {
    if (ai === b[j - 1]) {
      cur[j] = prev[j - 1] + 1;
    } else if (prev[j] >= cur[j - 1]) {
      cur[j] = prev[j];
    } else {
      cur[j] = cur[j - 1];
    }
  }
  prev = cur;
  if (i % 250 === 0) {
    console.log(prev[i]);
  }
}
