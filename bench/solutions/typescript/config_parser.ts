const lines = [
  "; settings", "title = demo", "[server]", "host = example.org", "port = 8080", 'name = "  main  "', "port = 9090",
  "[paths]", "root = /srv", "logs = ${paths.root}/logs", "data = ${root}/data", "bad line here", "[ server ]",
  "url = http://${host}:${port}/", "alias = ${missing}", "= value", "   # indented comment", "", "[paths]",
  "tags += web", "tags += api", "root = /var", 'quoted = "${root}"', "[extra]", "formula = x=y+1",
  "mirror = ${server.url}${paths.root}", "broken = ${paths.data}${server.nope}", 'half = "open',
];

const store = new Map<string, Map<string, string>>();
let section: string | null = null;

function lookup(sec: string, key: string): string | undefined {
  return store.get(sec)?.get(key);
}

lines.forEach((raw, i) => {
  const n = i + 1;
  const t = raw.trim();
  if (t === "" || t.startsWith(";") || t.startsWith("#")) return;
  if (t.startsWith("[") && t.endsWith("]")) {
    section = t.slice(1, -1).trim();
    return;
  }
  const eq = raw.indexOf("=");
  if (eq < 0) {
    console.log(`line ${n}: syntax error`);
    return;
  }
  let before = raw.slice(0, eq);
  let append = false;
  if (before.endsWith("+")) {
    append = true;
    before = before.slice(0, -1);
  }
  const key = before.trim();
  const value = raw.slice(eq + 1).trim();
  if (key === "") {
    console.log(`line ${n}: empty key`);
    return;
  }
  if (section === null) {
    console.log(`line ${n}: no section`);
    return;
  }
  const sec: string = section;
  let result: string;
  if (value.length >= 2 && value.startsWith('"') && value.endsWith('"')) {
    result = value.slice(1, -1);
  } else {
    result = "";
    let pos = 0;
    while (pos < value.length) {
      const start = value.indexOf("${", pos);
      if (start < 0) break;
      const end = value.indexOf("}", start + 2);
      if (end < 0) break;
      const ref = value.slice(start + 2, end);
      const dot = ref.indexOf(".");
      const v = dot >= 0 ? lookup(ref.slice(0, dot), ref.slice(dot + 1)) : lookup(sec, ref);
      if (v === undefined) {
        console.log(`line ${n}: unknown reference ${ref}`);
        return;
      }
      result += value.slice(pos, start) + v;
      pos = end + 1;
    }
    result += value.slice(pos);
  }
  if (!store.has(sec)) store.set(sec, new Map());
  const m = store.get(sec)!;
  if (append && m.has(key)) m.set(key, m.get(key)! + "," + result);
  else m.set(key, result);
});

console.log("---");
const cmp = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);
for (const s of [...store.keys()].sort(cmp)) {
  const m = store.get(s)!;
  for (const k of [...m.keys()].sort(cmp)) console.log(`${s}.${k} = [${m.get(k)}]`);
}
