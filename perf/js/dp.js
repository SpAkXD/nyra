// knapsack (one array) and LCS (a 2D table) (reference for dp.nyra)
// The generator is (seed * 1103515245 + 12345) mod 2^31; Math.imul keeps the low 32 bits exactly,
// which a double cannot do for the full product.
let seed = 42;
function next() {
  seed = (Math.imul(seed, 1103515245) + 12345) & 0x7fffffff;
  return seed;
}

function knapsack(w, v, cap) {
  const best = new Int32Array(cap + 1);
  for (let k = 0; k < w.length; k++) {
    for (let c = cap; c >= w[k]; c--) {
      const take = best[c - w[k]] + v[k];
      if (take > best[c]) best[c] = take;
    }
  }
  return best[cap];
}

function lcs(a, b) {
  const n = a.length, m = b.length;
  const dp = [];
  for (let i = 0; i <= n; i++) dp.push(new Int32Array(m + 1));
  for (let i = 1; i <= n; i++) {
    for (let j = 1; j <= m; j++) {
      if (a[i - 1] === b[j - 1]) dp[i][j] = dp[i - 1][j - 1] + 1;
      else dp[i][j] = dp[i - 1][j] > dp[i][j - 1] ? dp[i - 1][j] : dp[i][j - 1];
    }
  }
  return dp[n][m];
}

const w = [], v = [];
for (let k = 0; k < 1000; k++) {
  w.push(Math.floor(next() / 65536) % 1000 + 1);
  v.push(Math.floor(next() / 65536) % 1000 + 1);
}
console.log(String(knapsack(w, v, 50000)));
const a = [], b = [];
for (let k = 0; k < 2500; k++) {
  a.push(Math.floor(next() / 65536) % 4);
  b.push(Math.floor(next() / 65536) % 4);
}
console.log(String(lcs(a, b)));
