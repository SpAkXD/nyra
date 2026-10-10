term = "2333111"
for step in range(1, 41):
    parts = []
    i = 0
    n = len(term)
    while i < n:
        c = term[i]
        j = i
        while j < n and term[j] == c:
            j += 1
        parts.append(str(j - i))
        parts.append(c)
        i = j
    term = "".join(parts)
    if step % 5 == 0:
        print(step, len(term))
print(term.count("1"), term.count("2"), term.count("3"))
