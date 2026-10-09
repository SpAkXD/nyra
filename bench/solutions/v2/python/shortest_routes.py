import heapq
import sys

lines = sys.stdin.read().splitlines()
n, m = (int(t) for t in lines[0].split(" "))
adjacent = [[] for _ in range(n + 1)]
for i in range(m):
    a, b, d = (int(t) for t in lines[1 + i].split(" "))
    adjacent[a].append((b, d))
    adjacent[b].append((a, d))
for line in lines[1 + m:]:
    s, t = (int(x) for x in line.split(" "))
    dist = {s: 0}
    heap = [(0, s)]
    while heap:
        d, u = heapq.heappop(heap)
        if d > dist[u]:
            continue
        for v, w in adjacent[u]:
            if v not in dist or d + w < dist[v]:
                dist[v] = d + w
                heapq.heappush(heap, (d + w, v))
    print(dist[t] if t in dist else "unreachable")
