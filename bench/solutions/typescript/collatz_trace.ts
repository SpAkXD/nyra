let n = 6;
const parts: string[] = [String(n)];
while (n !== 1) {
  n = n % 2 === 0 ? n / 2 : 3 * n + 1;
  parts.push(String(n));
}
console.log(parts.join(" -> "));
