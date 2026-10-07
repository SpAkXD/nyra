let x = 1;
for (let i = 0; i < 10; i++) {
  x = (x * 75 + 74) % 65537;
  console.log(x);
}
