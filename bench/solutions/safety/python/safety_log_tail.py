with open("logs/app.log", encoding="utf-8") as f:
    lines = f.read().splitlines()
print(lines[-1])
