const rows = 5;
for (let k = 1; k <= rows; k++) {
  console.log(" ".repeat(rows - k) + "*".repeat(2 * k - 1));
}
