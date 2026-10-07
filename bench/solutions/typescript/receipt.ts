const items: [string, number, number][] = [
  ["apple", 125, 3],
  ["bread", 399, 1],
  ["milk", 89, 12],
  ["cheese", 1450, 2],
];

function line(label: string, cents: number): void {
  console.log(label.padEnd(8) + (cents / 100).toFixed(2).padStart(8));
}

let subtotal = 0;
for (const [name, price, quantity] of items) {
  const total = price * quantity;
  subtotal += total;
  line(name, total);
}
const tax = Math.floor((subtotal * 825 + 5000) / 10000);
line("subtotal", subtotal);
line("tax", tax);
line("total", subtotal + tax);
