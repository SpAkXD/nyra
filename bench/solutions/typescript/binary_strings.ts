function toBinary(n: number): string {
  let digits = "";
  while (n > 0) {
    digits = String(n % 2) + digits;
    n = Math.floor(n / 2);
  }
  return digits;
}

for (const n of [5, 10, 255, 1024]) {
  console.log(toBinary(n));
}
