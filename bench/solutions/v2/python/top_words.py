import re
import sys
from collections import Counter

lines = sys.stdin.read().splitlines()
k = int(lines[0])
counts = Counter(w.lower() for w in re.findall(r"[A-Za-z]+", "\n".join(lines[1:])))
if not counts:
    print("no words")
for word, n in sorted(counts.items(), key=lambda item: (-item[1], item[0]))[:k]:
    print(f"{word} {n}")
