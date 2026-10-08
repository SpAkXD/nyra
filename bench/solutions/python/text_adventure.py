OPPOSITE = {"north": "south", "south": "north", "east": "west", "west": "east", "down": "up", "up": "down"}
passages = [("hall", "north", "library"), ("hall", "east", "kitchen"), ("kitchen", "north", "pantry"),
            ("kitchen", "down", "cellar"), ("library", "east", "study"), ("study", "south", "pantry")]
exits = {}
for room, direction, target in passages:
    exits.setdefault(room, {})[direction] = target
    exits.setdefault(target, {})[OPPOSITE[direction]] = room
locked = {("library", "study"), ("study", "library")}
items = {"hall": ["lamp"], "pantry": ["key", "rope"], "study": ["book"], "cellar": ["coin"], "kitchen": [],
         "library": []}
commands = ("look; go north; unlock east; go east; go south; take lamp; go west; go east; go down; take coin; "
            "take coin; go up; go north; take key; drop coin; take key; inventory; go west; look; go north; "
            "unlock west; unlock west; go west; unlock east; drop lamp; go south; go east; go down; look; unlock up; "
            "take rope; go north; take coin; go south; go west; drop coin; inventory")

room = "hall"
carrying = []
visited = {"hall"}
for command in commands.split("; "):
    words = command.split(" ")
    verb = words[0]
    if verb == "go":
        d = words[1]
        target = exits[room].get(d)
        if target is None:
            print(f"no exit {d}")
        elif (room, target) in locked:
            print("the door is locked")
        elif target == "cellar" and "lamp" not in carrying:
            print("too dark to enter")
        else:
            room = target
            visited.add(room)
            print(f"you are in {room}")
    elif verb == "take":
        x = words[1]
        if x not in items[room]:
            print(f"no {x} here")
        elif len(carrying) >= 2:
            print("hands full")
        else:
            items[room].remove(x)
            carrying.append(x)
            print(f"taken {x}")
    elif verb == "drop":
        x = words[1]
        if x not in carrying:
            print(f"you have no {x}")
        else:
            carrying.remove(x)
            items[room].append(x)
            print(f"dropped {x}")
    elif verb == "unlock":
        d = words[1]
        target = exits[room].get(d)
        if target is None:
            print(f"no exit {d}")
        elif (room, target) not in locked:
            print("nothing to unlock")
        elif "key" not in carrying:
            print("you need the key")
        else:
            locked.discard((room, target))
            locked.discard((target, room))
            print("unlocked")
    elif verb == "look":
        here = ", ".join(sorted(items[room])) or "none"
        print(f"{room} | items: {here} | exits: {', '.join(sorted(exits[room]))}")
    elif verb == "inventory":
        print("carrying: " + (", ".join(sorted(carrying)) or "nothing"))

score = 10 * (len(visited) - 1) + (25 if "coin" in items["hall"] else 0)
print(f"score {score}")
