function survivor(n: number, k: number): number {
  const people: number[] = [];
  for (let i = 1; i <= n; i++) people.push(i);
  let index = 0;
  while (people.length > 1) {
    index = (index + k - 1) % people.length;
    people.splice(index, 1);
  }
  return people[0];
}

const cases: [number, number][] = [[7, 3], [41, 3], [10, 2]];
for (const [n, k] of cases) {
  console.log(survivor(n, k));
}
