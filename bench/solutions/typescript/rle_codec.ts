function encode(s: string): string {
  let out = "";
  let i = 0;
  while (i < s.length) {
    const c = s[i];
    let j = i;
    while (j < s.length && s[j] === c) j++;
    const n = j - i;
    if (c === "~") out += "~" + n + "~";
    else if (n >= 4) out += "~" + n + c;
    else out += c.repeat(n);
    i = j;
  }
  return out;
}
const isPlain = (c: string) => (c >= "a" && c <= "z") || c === ".";
const isDigit = (c: string) => c >= "0" && c <= "9";
function decode(code: string): string | null {
  let out = "";
  let i = 0;
  while (i < code.length) {
    const ch = code[i];
    if (ch === "~") {
      i++;
      if (i >= code.length || !isDigit(code[i]) || code[i] === "0") return null;
      let j = i;
      while (j < code.length && isDigit(code[j])) j++;
      const n = Number(code.slice(i, j));
      if (j >= code.length) return null;
      const c = code[j];
      if (!isPlain(c) && c !== "~") return null;
      out += c.repeat(n);
      i = j + 1;
    } else {
      if (!isPlain(ch)) return null;
      out += ch;
      i++;
    }
  }
  return out;
}
for (const t of ["aaaabbbcccccd", "~~x", ".......", "abc", "zzzzzzzzzzzzzz~", "a~~~~b"]) {
  console.log(`encode ${t} -> ${encode(t)}`);
}
for (const c of ["~4a~3~b", "~04a", "~2ab", "a~b", "~12z", "~5", "ab~3~~", "x~1~y", "q7", "~10.~4~", "~1~~1~", "a~3B"]) {
  const t = decode(c);
  if (t === null) console.log(`decode ${c} -> invalid`);
  else if (encode(t) === c) console.log(`decode ${c} -> ${t}`);
  else console.log(`decode ${c} -> ${t} (non-canonical)`);
}
