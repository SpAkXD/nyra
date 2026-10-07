const SYMBOLS: [number, string][] = [
  [1000, "M"], [900, "CM"], [500, "D"], [400, "CD"], [100, "C"], [90, "XC"],
  [50, "L"], [40, "XL"], [10, "X"], [9, "IX"], [5, "V"], [4, "IV"], [1, "I"],
];

function toRoman(n: number): string {
  let result = "";
  for (const [value, symbol] of SYMBOLS) {
    while (n >= value) {
      result += symbol;
      n -= value;
    }
  }
  return result;
}

for (const n of [4, 9, 14, 40, 90, 400, 1994, 2024]) {
  console.log(toRoman(n));
}
