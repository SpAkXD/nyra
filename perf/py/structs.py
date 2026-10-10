# array-of-structs updates: 100,000 particles bouncing in a box for 200 steps (reference for structs.nyra)
seed = 12345


def nxt():
    global seed
    seed = (seed * 1103515245 + 12345) % 2147483648
    return seed


class Particle:
    __slots__ = ("x", "y", "vx", "vy")

    def __init__(self, x, y, vx, vy):
        self.x, self.y, self.vx, self.vy = x, y, vx, vy


ps = []
for _ in range(100000):
    x = (nxt() // 65536) % 1000
    y = (nxt() // 65536) % 1000
    s = nxt()
    ps.append(Particle(x, y, (s // 65536) % 7 - 3, (s // 1024) % 5 - 2))
for _ in range(200):
    for p in ps:
        p.x += p.vx
        p.y += p.vy
        if p.x < 0 or p.x >= 1000:
            p.vx = -p.vx
        if p.y < 0 or p.y >= 1000:
            p.vy = -p.vy
sx = sy = 0
for p in ps:
    sx += p.x
    sy += p.y
print(f"{sx} {sy}")
