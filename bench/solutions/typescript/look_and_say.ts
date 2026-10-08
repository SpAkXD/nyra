let term = "2333111";
for (let step = 1; step <= 40; step++) {
  const parts: string[] = [];
  let i = 0;
  const n = term.length;
  while (i < n) {
    const c = term[i];
    let j = i;
    while (j < n && term[j] === c) {
      j++;
    }
    parts.push(String(j - i), c);
    i = j;
  }
  term = parts.join("");
  if (step % 5 === 0) {
    console.log(`${step} ${term.length}`);
  }
}
let ones = 0;
let twos = 0;
let threes = 0;
for (const c of term) {
  if (c === "1") ones++;
  else if (c === "2") twos++;
  else if (c === "3") threes++;
}
console.log(`${ones} ${twos} ${threes}`);
