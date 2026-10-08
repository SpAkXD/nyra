text = ("The rabbit's hole was deep; the rabbit didn't stop. 'Down, down, down!' said Alice -- and down-hill "
        "she went, past rabbit-holes, shelves, maps and jars. Was it 3 o'clock? It wasn't: the clock said "
        "half-past 4. ''Curious,'' thought Alice, ''curiouser'' -- and the rabbit's watch said nothing at all. "
        "DOWN went the jars, down went the maps; Alice didn't mind, didn't care, didn't stop. The rabbit's ears "
        "twitched; the rabbit whispered.")
STOP = set("the and for are but not you all any can had her was one our out his has".split())

counts = {}
word = ""
for ch in text.lower() + " ":
    if "a" <= ch <= "z" or ch == "'":
        word += ch
        continue
    w = word.strip("'")
    word = ""
    if len(w) < 3 or w in STOP:
        continue
    counts[w] = counts.get(w, 0) + 1

ranked = sorted(counts, key=lambda w: (-counts[w], w))
for w in ranked[:8]:
    n = str(counts[w])
    print(w + "." * (24 - len(w) - len(n)) + n)
print(f"distinct: {len(counts)}")
longest = min(counts, key=lambda w: (-len(w), w))
print(f"longest: {longest}")
