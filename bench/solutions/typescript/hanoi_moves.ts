// The iterative solution: every odd move takes the smallest disk one peg further around a cycle (A -> B -> C -> A
// for an even number of disks), every even move is the only legal move between the two other pegs.
const disks = 4;
const pegs: Record<string, number[]> = { A: [], B: [], C: [] };
for (let d = disks; d >= 1; d--) pegs.A.push(d);
const cycle = disks % 2 === 0 ? ["A", "B", "C"] : ["A", "C", "B"];
let smallestAt = 0;

function top(peg: string): number {
  const stack = pegs[peg];
  return stack.length > 0 ? stack[stack.length - 1] : Infinity;
}

function move(from: string, to: string): void {
  pegs[to].push(pegs[from].pop()!);
  console.log(`${from} -> ${to}`);
}

for (let m = 1; m < 2 ** disks; m++) {
  if (m % 2 === 1) {
    const from = cycle[smallestAt];
    smallestAt = (smallestAt + 1) % 3;
    move(from, cycle[smallestAt]);
  } else {
    const [x, y] = ["A", "B", "C"].filter((p) => p !== cycle[smallestAt]);
    if (top(x) < top(y)) move(x, y);
    else move(y, x);
  }
}
