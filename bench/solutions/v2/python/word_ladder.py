import sys
from collections import deque

lines = sys.stdin.read().splitlines()
start, end = lines[0].split(" ")
words = sorted(set(w for w in lines[1:] if w != ""))
if start == end:
    print(f"1 {start}")
    sys.exit(0)


def near(a, b):
    return len(a) == len(b) and sum(x != y for x, y in zip(a, b)) == 1


nodes = [start] + [w for w in words if w != start]
if end not in words:
    print("no ladder")
    sys.exit(0)
dist = {end: 0}
queue = deque([end])
while queue:
    u = queue.popleft()
    for v in nodes:
        if v not in dist and near(u, v):
            dist[v] = dist[u] + 1
            queue.append(v)
if start not in dist:
    print("no ladder")
else:
    path = [start]
    while path[-1] != end:
        cur = path[-1]
        path.append(min(v for v in words if v in dist and dist[v] == dist[cur] - 1 and near(cur, v)))
    print(f"{len(path)} " + " ".join(path))
