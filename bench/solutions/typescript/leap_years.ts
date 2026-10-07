function isLeap(year: number): boolean {
  return year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
}

for (const year of [1900, 1996, 2000, 2023, 2024, 2100]) {
  console.log(isLeap(year) ? "yes" : "no");
}
