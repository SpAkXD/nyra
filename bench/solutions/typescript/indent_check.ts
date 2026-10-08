const snippets: string[][] = [
  ["0|fn main() {", "4|let xs = [1, 2,", "4|3]", "4|if x {", "8|call(a, [b,", "8|c])", "4|}", "0|}"],
  ["0|f(a, {", "4|b: [1, 2)", "0|})"],
  ["0|g {", "2|h()", "0|}"],
  ["0|a {", "4|b {", "8|c", "4|}", "4|}"],
  ["0|x", "0|)"],
  ["0|k(", "4|m[", "8|n", "4|]"],
  ["2|a"],
  ["0|f() {", "0|}", "0|g [", "4|1, (2", "4|)]"],
  ["0|if a {", "4|b", "0|} else {", "4|c", "0|}"],
  ["0|p {", "3|q)"],
  ["0|r([", "4|s", "0|])", "0|t"],
];
const OPEN = "([{";
const CLOSE = ")]}";
function check(lines: string[]): string {
  const stack: { ch: string; line: number; indent: number }[] = [];
  let prevIndent = 0;
  let prevText = "";
  for (let li = 0; li < lines.length; li++) {
    const L = li + 1;
    const bar = lines[li].indexOf("|");
    const indent = Number(lines[li].slice(0, bar));
    const text = lines[li].slice(bar + 1);
    let firstCloseIndent = -1;
    for (let k = 0; k < text.length; k++) {
      const ch = text[k];
      if (OPEN.includes(ch)) stack.push({ ch, line: L, indent });
      else if (CLOSE.includes(ch)) {
        if (stack.length === 0) return `line ${L}: unexpected ${ch}`;
        const top = stack[stack.length - 1];
        const want = CLOSE[OPEN.indexOf(top.ch)];
        if (want !== ch) return `line ${L}: expected ${want} but found ${ch}`;
        stack.pop();
        if (k === 0) firstCloseIndent = top.indent;
      }
    }
    let expected: number;
    if (L === 1) expected = 0;
    else if (CLOSE.includes(text[0])) expected = firstCloseIndent;
    else if (OPEN.includes(prevText[prevText.length - 1])) expected = prevIndent + 4;
    else expected = prevIndent;
    if (indent !== expected) return `line ${L}: indent ${indent}, expected ${expected}`;
    prevIndent = indent;
    prevText = text;
  }
  if (stack.length > 0) {
    const top = stack[stack.length - 1];
    return `line ${top.line}: unclosed ${top.ch}`;
  }
  return "ok";
}
snippets.forEach((s, i) => console.log(`snippet ${i + 1}: ${check(s)}`));
