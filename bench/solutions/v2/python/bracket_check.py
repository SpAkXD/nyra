import sys

CLOSERS = {")": "(", "]": "[", "}": "{"}
for line in sys.stdin.read().splitlines():
    stack = []
    bad = 0
    for position, c in enumerate(line, 1):
        if c in "([{":
            stack.append((c, position))
        elif c in ")]}":
            if not stack or stack[-1][0] != CLOSERS[c]:
                bad = position
                break
            stack.pop()
    if bad == 0 and stack:
        bad = stack[-1][1]
    print("OK" if bad == 0 else f"ERROR at {bad}")
