const text = "the quick brown fox jumps over the lazy dog";
let count = 0;
for (const ch of text) {
  if ("aeiou".includes(ch)) count++;
}
console.log(count);
