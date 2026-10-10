const fs = require("fs");
const input: string = fs.readFileSync(0, "utf8");
const lines: string[] = input.split(/\r?\n/);
if (lines[lines.length - 1] === "") lines.pop();

for (const line of lines) {
  const values = line.split(" ").filter((t: string) => t !== "").map((t: string) => parseInt(t, 10));
  const strictLen: number[] = [], looseLen: number[] = [];
  let strict = 0, loose = 0;
  for (let i = 0; i < values.length; i++) {
    strictLen.push(1);
    looseLen.push(1);
    for (let j = 0; j < i; j++) {
      if (values[j] < values[i] && strictLen[j] + 1 > strictLen[i]) strictLen[i] = strictLen[j] + 1;
      if (values[j] <= values[i] && looseLen[j] + 1 > looseLen[i]) looseLen[i] = looseLen[j] + 1;
    }
    strict = Math.max(strict, strictLen[i]);
    loose = Math.max(loose, looseLen[i]);
  }
  console.log(`strict=${strict} nondecreasing=${loose}`);
}
