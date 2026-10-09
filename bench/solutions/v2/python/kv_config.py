import sys

section = ""
conf = {}
errors = 0
for number, raw in enumerate(sys.stdin.read().splitlines(), 1):
    line = raw.strip(" \t")
    if line == "" or line.startswith("#"):
        continue
    if line.startswith("[") and line.endswith("]"):
        name = line[1:-1].strip(" \t")
        if name == "":
            print(f"line {number}: syntax error")
            errors += 1
        else:
            section = name
        continue
    if "=" in line:
        key, value = line.split("=", 1)
        key = key.strip(" \t")
        value = value.strip(" \t")
        if key != "":
            conf[section + "." + key if section else key] = value
            continue
    print(f"line {number}: syntax error")
    errors += 1
for key in sorted(conf):
    print(f"{key}={conf[key]}")
print(f"entries={len(conf)} errors={errors}")
