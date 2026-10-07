COMMANDS = ("open A 100; open B 50; open A 10; deposit C 5; withdraw B 70; transfer A B 60; transfer B A 112; "
            "transfer B A 109; transfer A A 5; withdraw A 25; close B; transfer A B 1; deposit A 7; withdraw A 15; "
            "close A; open B 0; close B")

accounts = {}
for command in COMMANDS.split("; "):
    words = command.split()
    op = words[0]
    if op == "open":
        name, amount = words[1], int(words[2])
        if name in accounts:
            print(f"{name} exists")
        else:
            accounts[name] = amount
            print(f"opened {name}")
    elif op == "deposit":
        name, amount = words[1], int(words[2])
        if name not in accounts:
            print(f"{name} unknown")
        else:
            accounts[name] += amount
            print(f"{name} balance {accounts[name]}")
    elif op == "withdraw":
        name, amount = words[1], int(words[2])
        if name not in accounts:
            print(f"{name} unknown")
        elif accounts[name] - amount < 0:
            print(f"{name} insufficient")
        else:
            accounts[name] -= amount
            print(f"{name} balance {accounts[name]}")
    elif op == "transfer":
        source, target, amount = words[1], words[2], int(words[3])
        if source not in accounts or target not in accounts:
            print("unknown account")
        elif accounts[source] < amount + 1:
            print(f"{source} insufficient")
        else:
            accounts[source] -= amount + 1
            accounts[target] += amount
            print(f"{source} balance {accounts[source]} {target} balance {accounts[target]}")
    elif op == "close":
        name = words[1]
        if name not in accounts:
            print(f"{name} unknown")
        elif accounts[name] != 0:
            print(f"{name} not empty")
        else:
            del accounts[name]
            print(f"closed {name}")
