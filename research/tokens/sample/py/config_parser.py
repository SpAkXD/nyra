import re

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

data = {}
section = None
out = []
pat = re.compile(r'\$\{([^}]*)\}')

for n, raw in enumerate(lines, 1):
    s = raw.strip(" ")
    if s == "" or s[0] in ";#":
        continue
    if s.startswith("[") and s.endswith("]"):
        section = s[1:-1].strip(" ")
        continue
    if "=" not in raw:
        out.append(f"line {n}: syntax error")
        continue
    i = raw.index("=")
    left = raw[:i]
    append = False
    if i > 0 and raw[i - 1] == "+":
        append = True
        left = raw[:i - 1]
    key = left.strip(" ")
    val = raw[i + 1:].strip(" ")
    if key == "":
        out.append(f"line {n}: empty key")
        continue
    if section is None:
        out.append(f"line {n}: no section")
        continue
    if len(val) >= 2 and val[0] == '"' and val[-1] == '"':
        newval = val[1:-1]
    else:
        err = None
        def rep(m):
            global err
            r = m.group(1)
            if "." in r:
                sname, k = r.split(".", 1)
            else:
                sname, k = section, r
            if sname in data and k in data[sname]:
                return data[sname][k]
            if err is None:
                err = r
            return ""
        newval = pat.sub(rep, val)
        if err is not None:
            out.append(f"line {n}: unknown reference {err}")
            continue
    d = data.setdefault(section, {})
    if append and key in d:
        d[key] = d[key] + "," + newval
    else:
        d[key] = newval

for o in out:
    print(o)
print("---")
for sname in sorted(data):
    for k in sorted(data[sname]):
        print(f"{sname}.{k} = [{data[sname][k]}]")
