import sys

tokens = sys.stdin.read().split()
n, m = int(tokens[0]), int(tokens[1])
parent = list(range(n + 1))


def find(x):
    while parent[x] != x:
        parent[x] = parent[parent[x]]
        x = parent[x]
    return x


for i in range(m):
    a, b = int(tokens[2 + 2 * i]), int(tokens[3 + 2 * i])
    parent[find(a)] = find(b)
sizes = {}
smallest = {}
for node in range(1, n + 1):
    root = find(node)
    sizes[root] = sizes.get(root, 0) + 1
    smallest.setdefault(root, node)
order = sorted(sizes, key=lambda r: (-sizes[r], smallest[r]))
print(f"components={len(order)}")
print("sizes: " + " ".join(str(sizes[r]) for r in order))
print(f"largest contains node {smallest[order[0]]}")
