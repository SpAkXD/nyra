// quicksort and the built-in sort of 1,000,000 ints (reference for sort.nyra)
function quicksort(xs, lo, hi) {
  if (lo >= hi) return;
  const pivot = xs[Math.floor((lo + hi) / 2)];
  let i = lo, j = hi;
  while (i <= j) {
    while (xs[i] < pivot) i++;
    while (xs[j] > pivot) j--;
    if (i <= j) {
      const t = xs[i]; xs[i] = xs[j]; xs[j] = t;
      i++; j--;
    }
  }
  quicksort(xs, lo, j);
  quicksort(xs, i, hi);
}

const n = 1000000;
let seed = 7;
const xs = new Float64Array(n);
for (let k = 0; k < n; k++) {
  seed = (Math.imul(seed, 1103515245) + 12345) & 0x7fffffff;
  xs[k] = Math.floor(seed / 64) % 1000000;
}
const ys = Float64Array.from(xs);
quicksort(xs, 0, n - 1);
ys.sort();
let same = true;
for (let i = 0; i < n; i++) if (xs[i] !== ys[i]) { same = false; break; }
console.log(same ? "true" : "false");
let check = 0;
for (let i = 0; i < n; i++) check = (check * 31 + xs[i]) % 1000000007;
console.log(String(check));
