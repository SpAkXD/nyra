const days = 31;
const first = 2; // Monday is 0, so Wednesday is 2

console.log("Mo Tu We Th Fr Sa Su");
for (let row = 0; 7 * row - first + 1 <= days; row++) {
  const cells: string[] = [];
  for (let col = 0; col < 7; col++) {
    const day = 7 * row + col - first + 1;
    cells.push(day >= 1 && day <= days ? String(day).padStart(2) : "  ");
  }
  console.log(cells.join(" ").trimEnd());
}
