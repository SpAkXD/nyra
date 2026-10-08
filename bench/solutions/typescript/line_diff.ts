function diff(a: string[], b: string[]): void {
  const n = a.length;
  const m = b.length;
  const L: number[][] = [];
  for (let i = 0; i <= n; i++) L.push(new Array(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      L[i][j] = a[i] === b[j] ? L[i + 1][j + 1] + 1 : Math.max(L[i + 1][j], L[i][j + 1]);
    }
  }
  const script: [string, string][] = [];
  let i = 0;
  let j = 0;
  while (i < n || j < m) {
    if (i < n && j < m && a[i] === b[j]) { script.push(["=", a[i]]); i++; j++; }
    else if (i < n && (j === m || L[i + 1][j] >= L[i][j + 1])) { script.push(["-", a[i]]); i++; }
    else { script.push(["+", b[j]]); j++; }
  }
  let ins = 0, del = 0, same = 0;
  let k = 0;
  while (k < script.length) {
    const [op, line] = script[k];
    if (op === "=") {
      let e = k;
      while (e < script.length && script[e][0] === "=") e++;
      const run = e - k;
      same += run;
      if (run >= 3) console.log(`= (${run} unchanged)`);
      else for (let x = k; x < e; x++) console.log(`= ${script[x][1]}`);
      k = e;
    } else {
      if (op === "+") ins++; else del++;
      console.log(`${op} ${line}`);
      k++;
    }
  }
  console.log(`summary: +${ins} -${del} =${same}`);
}
console.log("diff 1");
diff(["alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa"],
  ["alpha", "gamma", "delta", "beta", "epsilon", "zeta", "eta", "lambda", "theta", "kappa", "mu"]);
console.log("diff 2");
diff(["x = 1", "y = 2", "print(x)", "print(y)", "z = x + y", "print(z)"],
  ["y = 2", "x = 1", "print(x)", "z = x + y", "print(z)", "print(y)"]);
