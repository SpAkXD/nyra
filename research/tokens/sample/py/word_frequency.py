import re
from collections import Counter

text = """The rabbit's hole was deep; the rabbit didn't stop. 'Down, down, down!' said Alice -- and down-hill she went, past rabbit-holes, shelves, maps and jars. Was it 3 o'clock? It wasn't: the clock said half-past 4. ''Curious,'' thought Alice, ''curiouser'' -- and the rabbit's watch said nothing at all. DOWN went the jars, down went the maps; Alice didn't mind, didn't care, didn't stop. The rabbit's ears twitched; the rabbit whispered."""

stop = set("the and for are but not you all any can had her was one our out his has".split())

words = []
for cand in re.findall(r"[a-z']+", text.lower()):
    w = cand.strip("'")
    if len(w) < 3 or w in stop:
        continue
    words.append(w)

c = Counter(words)
items = sorted(c.items(), key=lambda kv: (-kv[1], kv[0]))
for w, n in items[:8]:
    s = str(n)
    print(w + "." * (24 - len(w) - len(s)) + s)
print(f"distinct: {len(c)}")
longest = sorted(c.keys(), key=lambda w: (-len(w), w))[0]
print(f"longest: {longest}")
