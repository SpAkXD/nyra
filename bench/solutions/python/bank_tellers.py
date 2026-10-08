customers = [("ann", 0, 5, "regular"), ("bob", 0, 3, "regular"), ("cy", 1, 4, "vip"), ("dee", 2, 6, "regular"),
             ("eli", 2, 2, "vip"), ("fay", 3, 4, "regular"), ("gus", 5, 1, "regular"), ("hal", 18, 9, "regular"),
             ("ivy", 19, 3, "vip"), ("jon", 20, 4, "regular"), ("kim", 20, 5, "vip"), ("lou", 22, 4, "regular"),
             ("max", 27, 1, "regular"), ("ned", 30, 2, "vip")]
TELLERS = 3
BREAK_TELLER, BREAK_START, BREAK_END = 3, 20, 29

busy_until = [None] * (TELLERS + 1)  # index 1..3: minute the current service ends, or None
line = []  # (name, arrival, service, kind)
log = []
total_wait = 0
longest = None  # (wait, list index, name)
done = 0
minute = 0
order = {c[0]: i for i, c in enumerate(customers)}
while done < len(customers):
    for t in range(1, TELLERS + 1):
        if busy_until[t] == minute:
            busy_until[t] = None
    for c in customers:
        if c[1] == minute:
            if c[3] == "vip":
                pos = 0
                while pos < len(line) and line[pos][3] == "vip":
                    pos += 1
                line.insert(pos, c)
            else:
                line.append(c)
    for t in range(1, TELLERS + 1):
        if not line:
            break
        if busy_until[t] is not None:
            continue
        if t == BREAK_TELLER and BREAK_START <= minute <= BREAK_END:
            continue
        name, arrival, service, _ = line.pop(0)
        busy_until[t] = minute + service
        wait = minute - arrival
        total_wait += wait
        if longest is None or wait > longest[0] or (wait == longest[0] and order[name] < longest[1]):
            longest = (wait, order[name], name)
        print(f"{minute} {name} teller {t} wait {wait} end {minute + service}")
        done += 1
    minute += 1
print(f"total wait {total_wait}, longest wait {longest[0]} ({longest[2]})")
