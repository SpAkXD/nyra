let result = 1;
for (let i = 0; i < 200; i++) {
  result = (result * 3) % 1000007;
}
console.log(result);
