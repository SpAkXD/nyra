// choose the number of 50, 25 and 10 cent coins; then any number of 5 cent coins fits and 1 cent coins fill the rest
const amount = 175;
let count = 0;
for (let halves = 0; 50 * halves <= amount; halves++) {
  for (let quarters = 0; 50 * halves + 25 * quarters <= amount; quarters++) {
    for (let dimes = 0; 50 * halves + 25 * quarters + 10 * dimes <= amount; dimes++) {
      const rest = amount - 50 * halves - 25 * quarters - 10 * dimes;
      count += Math.floor(rest / 5) + 1;
    }
  }
}
console.log(count);
