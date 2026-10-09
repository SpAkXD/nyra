import sys

inside = {}
revenue = 0
for number, line in enumerate(sys.stdin.read().splitlines(), 1):
    when, kind, plate = line.split(" ")
    minutes = int(when[:2]) * 60 + int(when[3:])
    if kind == "IN":
        if plate in inside:
            print(f"line {number}: already inside")
        else:
            inside[plate] = minutes
    else:
        if plate not in inside:
            print(f"line {number}: not inside")
        else:
            stay = minutes - inside.pop(plate)
            fee = 0 if stay <= 15 else min(20, 2 * ((stay + 59) // 60))
            revenue += fee
            print(f"{plate} paid ${fee}")
print(f"revenue=${revenue}")
print("still inside: " + (",".join(sorted(inside)) if inside else "none"))
