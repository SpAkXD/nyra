import sys

seen = set()
groups = {}
for raw in sys.stdin.read().splitlines():
    word = raw.strip(" ")
    if word == "" or word.lower() in seen:
        continue
    seen.add(word.lower())
    groups.setdefault("".join(sorted(word.lower())), []).append(word)
result = [sorted(words, key=str.lower) for words in groups.values() if len(words) >= 2]
result.sort(key=lambda words: words[0].lower())
if result:
    for words in result:
        print(" ".join(words))
else:
    print("no anagrams")
