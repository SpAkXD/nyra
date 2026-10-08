jobs = [("A", 0, 5), ("B", 1, 3), ("C", 3, 8), ("D", 11, 2), ("E", 30, 4), ("F", 30, 1)]
SLICE = 3

remaining = {name: need for name, _, need in jobs}
finish = {}
queue = []
joined = 0  # jobs[:joined] have joined the queue
t = 0
last = None  # job of the previous slice, None after an idle period or at the start
interrupted = None


def join_arrivals(now):
    global joined
    while joined < len(jobs) and jobs[joined][1] <= now:
        queue.append(jobs[joined][0])
        joined += 1


while len(finish) < len(jobs):
    join_arrivals(t)
    if interrupted is not None:
        queue.append(interrupted)
        interrupted = None
    if not queue:
        next_arrival = jobs[joined][1]
        print(f"{t}-{next_arrival} idle")
        t = next_arrival
        last = None
        continue
    job = queue.pop(0)
    if last is not None and last != job:
        print(f"{t}-{t + 1} switch")
        t += 1
    run = min(SLICE, remaining[job])
    print(f"{t}-{t + run} {job}")
    t += run
    remaining[job] -= run
    last = job
    if remaining[job] == 0:
        finish[job] = t
    else:
        interrupted = job

for name, arrival, need in jobs:
    print(f"{name} finish {finish[name]} wait {finish[name] - arrival - need}")
