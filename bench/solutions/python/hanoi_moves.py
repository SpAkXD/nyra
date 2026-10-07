def hanoi(disks, source, target, spare):
    if disks == 0:
        return
    hanoi(disks - 1, source, spare, target)
    print(f"{source} -> {target}")
    hanoi(disks - 1, spare, target, source)


hanoi(4, "A", "C", "B")
