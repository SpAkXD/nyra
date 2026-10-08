lines = [
    "; settings",
    "title = demo",
    "[server]",
    "host = example.org",
    "port = 8080",
    'name = "  main  "',
    "port = 9090",
    "[paths]",
    "root = /srv",
    "logs = ${paths.root}/logs",
    "data = ${root}/data",
    "bad line here",
    "[ server ]",
    "url = http://${host}:${port}/",
    "alias = ${missing}",
    "= value",
    "   # indented comment",
    "",
    "[paths]",
    "tags += web",
    "tags += api",
    "root = /var",
    'quoted = "${root}"',
    "[extra]",
    "formula = x=y+1",
    "mirror = ${server.url}${paths.root}",
    "broken = ${paths.data}${server.nope}",
    'half = "open',
]

settings = {}  # (section, key) -> value
section = None
for number, raw in enumerate(lines, 1):
    line = raw.strip()
    if line == "" or line[0] in ";#":
        continue
    if line.startswith("[") and line.endswith("]"):
        section = line[1:-1].strip()
        continue
    if "=" not in line:
        print(f"line {number}: syntax error")
        continue
    eq = line.index("=")
    key_part, value = line[:eq], line[eq + 1:].strip()
    append = key_part.endswith("+")
    if append:
        key_part = key_part[:-1]
    key = key_part.strip()
    if key == "":
        print(f"line {number}: empty key")
        continue
    if section is None:
        print(f"line {number}: no section")
        continue
    if len(value) >= 2 and value[0] == '"' and value[-1] == '"':
        value = value[1:-1]
    else:
        out = ""
        rest = value
        bad = None
        while "${" in rest:
            start = rest.index("${")
            end = rest.index("}", start)
            ref = rest[start + 2:end]
            target = tuple(ref.split(".", 1)) if "." in ref else (section, ref)
            if target not in settings:
                bad = ref
                break
            out += rest[:start] + settings[target]
            rest = rest[end + 1:]
        if bad is not None:
            print(f"line {number}: unknown reference {bad}")
            continue
        value = out + rest
    if append and (section, key) in settings:
        value = settings[(section, key)] + "," + value
    settings[(section, key)] = value

print("---")
for (sec, key) in sorted(settings):
    print(f"{sec}.{key} = [{settings[(sec, key)]}]")
