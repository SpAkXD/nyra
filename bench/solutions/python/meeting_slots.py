PEOPLE = ["ann", "bob", "cat", "dan"]
busy_text = {
    "ann": "09:00-10:30, 12:00-13:00, 15:45-17:00",
    "bob": "09:30-11:15, 13:00-14:00",
    "cat": "11:00-12:15, 14:30-15:15",
    "dan": "10:00-10:45, 13:30-16:00",
}
requests = "ann+bob 60; bob+cat+dan 30; ann+cat 45; ann+bob+cat+dan 30; dan 120; cat+dan 15; ann+bob 45; bob 120"
DAY_START, DAY_END = 9 * 60, 17 * 60


def minutes(hhmm):
    h, m = hhmm.split(":")
    return int(h) * 60 + int(m)


def clock(t):
    return f"{t // 60:02d}:{t % 60:02d}"


busy = {p: [] for p in PEOPLE}
for p, text in busy_text.items():
    for period in text.split(", "):
        s, e = period.split("-")
        busy[p].append((minutes(s), minutes(e)))


def free(p, s, e):
    return all(e <= bs or s >= be for bs, be in busy[p])


for request in requests.split("; "):
    names_text, duration_text = request.split(" ")
    names = names_text.split("+")
    duration = int(duration_text)
    found = None
    start = DAY_START
    while start + duration <= DAY_END:
        if all(free(p, start, start + duration) for p in names):
            found = start
            break
        start += 15
    if found is None:
        print(f"{names_text}: no slot")
        continue
    for p in names:
        busy[p].append((found, found + duration))
    print(f"{names_text}: {clock(found)}-{clock(found + duration)}")

for p in PEOPLE:
    periods = []
    t = DAY_START
    for s, e in sorted(busy[p]):
        if s > t:
            periods.append(f"{clock(t)}-{clock(s)}")
        t = max(t, e)
    if t < DAY_END:
        periods.append(f"{clock(t)}-{clock(DAY_END)}")
    print(f"{p} free: {', '.join(periods) or 'none'}")
