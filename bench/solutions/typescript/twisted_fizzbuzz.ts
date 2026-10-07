function hasSeven(n: number): boolean {
  for (let x = n; x > 0; x = Math.floor(x / 10)) {
    if (x % 10 === 7) return true;
  }
  return false;
}

for (let n = 1; n <= 40; n++) {
  if (hasSeven(n)) console.log("Seven");
  else if (n % 12 === 0) console.log("Twelve");
  else if (n % 3 === 0) console.log("Three");
  else if (n % 4 === 0) console.log("Four");
  else console.log(n);
}
