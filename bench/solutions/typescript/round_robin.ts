const jobs = [["A", 0, 5], ["B", 1, 3], ["C", 3, 8], ["D", 11, 2], ["E", 30, 4], ["F", 30, 1]] as [string, number, number][];
const remaining = jobs.map((j) => j[2]);
const finish: number[] = jobs.map(() => 0);
const joined = jobs.map(() => false);
const queue: number[] = [];
let t = 0;
let prev = -1;
let idleSince = true;
let carry = -1;
while (true) {
  for (let i = 0; i < jobs.length; i++) {
    if (!joined[i] && jobs[i][1] <= t) { joined[i] = true; queue.push(i); }
  }
  if (carry >= 0) { queue.push(carry); carry = -1; }
  if (queue.length === 0) {
    let next = Infinity;
    for (let i = 0; i < jobs.length; i++) if (!joined[i]) next = Math.min(next, jobs[i][1]);
    if (next === Infinity) break;
    console.log(`${t}-${next} idle`);
    t = next;
    idleSince = true;
    continue;
  }
  const j = queue.shift()!;
  if (prev >= 0 && prev !== j && !idleSince) {
    console.log(`${t}-${t + 1} switch`);
    t += 1;
  }
  const run = Math.min(3, remaining[j]);
  console.log(`${t}-${t + run} ${jobs[j][0]}`);
  t += run;
  remaining[j] -= run;
  prev = j;
  idleSince = false;
  if (remaining[j] > 0) carry = j;
  else finish[j] = t;
}
for (let i = 0; i < jobs.length; i++) {
  console.log(`${jobs[i][0]} finish ${finish[i]} wait ${finish[i] - jobs[i][1] - jobs[i][2]}`);
}
