// array-of-structs updates: 100,000 particles bouncing in a box for 200 steps (reference for structs.nyra)
let seed = 12345;
function next() {
  seed = (Math.imul(seed, 1103515245) + 12345) & 0x7fffffff;
  return seed;
}
const n = 100000;
const ps = [];
for (let i = 0; i < n; i++) {
  const x = Math.floor(next() / 65536) % 1000;
  const y = Math.floor(next() / 65536) % 1000;
  const s = next();
  ps.push({ x, y, vx: Math.floor(s / 65536) % 7 - 3, vy: Math.floor(s / 1024) % 5 - 2 });
}
for (let step = 0; step < 200; step++) {
  for (let i = 0; i < n; i++) {
    const p = ps[i];
    p.x += p.vx;
    p.y += p.vy;
    if (p.x < 0 || p.x >= 1000) p.vx = -p.vx;
    if (p.y < 0 || p.y >= 1000) p.vy = -p.vy;
  }
}
let sx = 0, sy = 0;
for (const p of ps) { sx += p.x; sy += p.y; }
console.log(`${sx} ${sy}`);
