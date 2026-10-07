def ack(m, n):
    if m == 0:
        return n + 1
    if n == 0:
        return ack(m - 1, 1)
    return ack(m - 1, ack(m, n - 1))


for m, n in ((0, 0), (1, 2), (2, 3), (3, 3), (3, 5)):
    print(ack(m, n))
