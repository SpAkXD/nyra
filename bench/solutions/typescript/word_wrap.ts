const text = "the quick brown fox jumps over the lazy dog and keeps running far away from here";
const width = 18;

const lines: string[][] = [[]];
for (const word of text.split(" ")) {
  const current = lines[lines.length - 1];
  const length = current.reduce((sum, w) => sum + w.length + 1, 0) + word.length;
  if (current.length > 0 && length > width) {
    lines.push([word]);
  } else {
    current.push(word);
  }
}
for (const words of lines) {
  console.log(words.join(" "));
}
console.log(`lines: ${lines.length}`);
