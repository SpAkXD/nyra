const pos: number[][] = [[-7, 17, -11], [9, 12, 5], [-9, 0, -4], [4, 6, 0], [1, -13, 8]];
const vel: number[][] = pos.map(() => [0, 0, 0]);

function energy(): number {
  let total = 0;
  for (let b = 0; b < 5; b++) {
    const p = Math.abs(pos[b][0]) + Math.abs(pos[b][1]) + Math.abs(pos[b][2]);
    const k = Math.abs(vel[b][0]) + Math.abs(vel[b][1]) + Math.abs(vel[b][2]);
    total += p * k;
  }
  return total;
}

for (let step = 1; step <= 20000; step++) {
  for (let a = 0; a < 5; a++) {
    for (let b = a + 1; b < 5; b++) {
      for (let axis = 0; axis < 3; axis++) {
        if (pos[a][axis] < pos[b][axis]) {
          vel[a][axis]++;
          vel[b][axis]--;
        } else if (pos[a][axis] > pos[b][axis]) {
          vel[a][axis]--;
          vel[b][axis]++;
        }
      }
    }
  }
  for (let b = 0; b < 5; b++) {
    for (let axis = 0; axis < 3; axis++) {
      pos[b][axis] += vel[b][axis];
    }
  }
  if (step % 4000 === 0) {
    console.log(energy());
  }
}
for (let b = 0; b < 5; b++) {
  console.log([...pos[b], ...vel[b]].join(" "));
}
