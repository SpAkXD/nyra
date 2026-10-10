pos = [[-7, 17, -11], [9, 12, 5], [-9, 0, -4], [4, 6, 0], [1, -13, 8]]
vel = [[0, 0, 0] for _ in range(5)]


def energy():
    total = 0
    for b in range(5):
        p = abs(pos[b][0]) + abs(pos[b][1]) + abs(pos[b][2])
        k = abs(vel[b][0]) + abs(vel[b][1]) + abs(vel[b][2])
        total += p * k
    return total


for step in range(1, 20001):
    for a in range(5):
        for b in range(a + 1, 5):
            for axis in range(3):
                if pos[a][axis] < pos[b][axis]:
                    vel[a][axis] += 1
                    vel[b][axis] -= 1
                elif pos[a][axis] > pos[b][axis]:
                    vel[a][axis] -= 1
                    vel[b][axis] += 1
    for b in range(5):
        for axis in range(3):
            pos[b][axis] += vel[b][axis]
    if step % 4000 == 0:
        print(energy())
for b in range(5):
    print(pos[b][0], pos[b][1], pos[b][2], vel[b][0], vel[b][1], vel[b][2])
