let population = 1000;
for (let year = 1; year <= 10; year++) {
  population += Math.floor((population * 10) / 100);
  population -= 50;
  console.log(population);
}
