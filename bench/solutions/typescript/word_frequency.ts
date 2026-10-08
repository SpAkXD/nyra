const text = "The rabbit's hole was deep; the rabbit didn't stop. 'Down, down, down!' said Alice -- and down-hill she went, past rabbit-holes, shelves, maps and jars. Was it 3 o'clock? It wasn't: the clock said half-past 4. ''Curious,'' thought Alice, ''curiouser'' -- and the rabbit's watch said nothing at all. DOWN went the jars, down went the maps; Alice didn't mind, didn't care, didn't stop. The rabbit's ears twitched; the rabbit whispered.";

const stop = new Set("the and for are but not you all any can had her was one our out his has".split(" "));
const counts = new Map<string, number>();
for (const cand of text.toLowerCase().match(/[a-z']+/g) ?? []) {
  const w = cand.replace(/^'+/, "").replace(/'+$/, "");
  if (w.length < 3 || stop.has(w)) continue;
  counts.set(w, (counts.get(w) ?? 0) + 1);
}

const cmp = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);
const words = [...counts.keys()].sort((a, b) => counts.get(b)! - counts.get(a)! || cmp(a, b));
for (const w of words.slice(0, 8)) {
  const c = String(counts.get(w));
  console.log(w + ".".repeat(24 - w.length - c.length) + c);
}
console.log(`distinct: ${counts.size}`);
const longest = [...counts.keys()].sort(cmp).reduce((best, w) => (w.length > best.length ? w : best), "");
console.log(`longest: ${longest}`);
