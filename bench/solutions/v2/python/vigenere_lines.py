import sys

lines = sys.stdin.read().splitlines()
key = [ord(c) - 97 for c in lines[0]]
for line in lines[1:]:
    if len(line) < 2 or line[0] not in "ED" or line[1] != " ":
        print("error")
        continue
    sign = 1 if line[0] == "E" else -1
    out = []
    i = 0
    for c in line[2:]:
        if "a" <= c <= "z" or "A" <= c <= "Z":
            base = 97 if c.islower() else 65
            out.append(chr((ord(c) - base + sign * key[i % len(key)]) % 26 + base))
            i += 1
        else:
            out.append(c)
    print("".join(out))
