import heapq
import sys

after = {}
indegree = {}
for line in sys.stdin.read().splitlines():
    if line == "":
        continue
    a, b = line.split(" -> ")
    indegree.setdefault(a, 0)
    indegree.setdefault(b, 0)
    after.setdefault(a, []).append(b)
    indegree[b] += 1
ready = [t for t in indegree if indegree[t] == 0]
heapq.heapify(ready)
order = []
while ready:
    t = heapq.heappop(ready)
    order.append(t)
    for u in after.get(t, []):
        indegree[u] -= 1
        if indegree[u] == 0:
            heapq.heappush(ready, u)
print(" ".join(order) if len(order) == len(indegree) else "cycle")
