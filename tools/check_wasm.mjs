// Checks the WebAssembly build (tools/build_wasm.py) against the native compiler, in Node.
//
//   node tools/check_wasm.mjs                              every examples/*.nyra with an .out file
//   node tools/check_wasm.mjs --site ../site-play          also the playground's examples (site/play/examples.js)
//   node tools/check_wasm.mjs --wasm path/to/nyra.wasm     another build
//
// examples/*.nyra: the output of `run_interp` (stdin from the .in file) must equal the .out file.
// The playground's examples: `run_interp` must print what `nyra run --sandbox --allow input` prints
// (target/release/nyra), and stop with the same error code when it fails.
import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const arg = (name) => { const i = process.argv.indexOf(name); return i > 0 ? process.argv[i + 1] : undefined; };
const wasmPath = arg("--wasm") || path.join(root, "target/wasm32-unknown-unknown/wasm/nyra.wasm");
const site = arg("--site");
const native = path.join(root, "target/release/nyra" + (process.platform === "win32" ? ".exe" : ""));

// ---- the C ABI (the same as site/play/nyra.js) ----
const module = new WebAssembly.Module(fs.readFileSync(wasmPath));
const env = { nyra_now_ms: () => Date.now(), nyra_mono_ms: () => performance.now(), nyra_random: () => Math.random() };
let ex;
const fresh = () => { ex = new WebAssembly.Instance(module, { env }).exports; };
fresh();
const enc = new TextEncoder(), dec = new TextDecoder();
function call(name, ...texts) {
  const bufs = texts.map((t) => {
    const b = enc.encode(t), p = ex.nyra_alloc(b.length);
    new Uint8Array(ex.memory.buffer, p, b.length).set(b);
    return [p, b.length];
  });
  try {
    const r = ex[name](...bufs.flat());
    return dec.decode(new Uint8Array(ex.memory.buffer, r, ex.result_len()).slice());
  } catch (e) {
    fresh(); // a trap leaves the instance in an unknown state
    throw e;
  } finally {
    try { for (const [p, n] of bufs) ex.nyra_free(p, n); } catch (_) { /* a new instance */ }
  }
}
const run = (src, stdin = "", limits = "") => JSON.parse(call("run_interp", src, stdin, limits));

let failed = 0, passed = 0;
const fail = (what, msg) => { failed++; console.log(`FAIL ${what}: ${msg}`); };
const norm = (s) => s.replace(/\r\n/g, "\n");

console.log(`nyra.wasm ${call("version")}: ${fs.statSync(wasmPath).size.toLocaleString("en")} bytes, exports ${WebAssembly.Module.exports(module).map((e) => e.name).join(" ")}`);

// ---- 1. the examples of the repository ----
const dir = path.join(root, "examples");
for (const f of fs.readdirSync(dir).filter((f) => f.endsWith(".nyra")).sort()) {
  const base = path.join(dir, f.slice(0, -5));
  if (!fs.existsSync(base + ".out") || fs.existsSync(base + ".args")) continue;
  const src = fs.readFileSync(base + ".nyra", "utf8");
  // only what the playground grants: programs that need files or the OS are skipped
  if (/^use (fs|os)\b/m.test(src) || /^use \.\//m.test(src)) continue;
  const stdin = fs.existsSync(base + ".in") ? fs.readFileSync(base + ".in", "utf8") : "";
  const want = norm(fs.readFileSync(base + ".out", "utf8"));
  let got;
  try {
    got = run(src, stdin, "fuel=2000000000 time=60000 depth=500 output=64M memory=1G");
  } catch (e) {
    fail(f, `threw ${e.message}`);
    continue;
  }
  const exitWant = fs.existsSync(base + ".exit") ? Number(fs.readFileSync(base + ".exit", "utf8").trim()) : 0;
  if (got.stdout !== want) fail(f, `stdout differs\n--- want\n${want}--- got\n${got.stdout}${JSON.stringify(got.errors || got.stderr || "")}`);
  else if (got.exit !== exitWant) fail(f, `exit ${got.exit}, want ${exitWant}: ${JSON.stringify(got.errors)}`);
  else passed++;
}
console.log(`examples/: ${passed} passed`);

// ---- 2. the playground's examples, against the native compiler ----
if (site) {
  const { EXAMPLES } = await import(pathToFileURL(path.resolve(site, "site/play/examples.js")).href);
  const tmp = fs.mkdtempSync(path.join(root, ".tmp-play-"));
  let n = 0;
  for (const e of EXAMPLES) {
    const file = path.join(tmp, "main.nyra");
    fs.writeFileSync(file, e.code);
    const nat = spawnSync(native, ["run", file, "--sandbox", "--allow", "input", "--json"], { input: e.stdin || "", encoding: "utf8", cwd: tmp });
    const got = run(e.code, e.stdin || "");
    const natOut = norm(nat.stdout);
    // with --json a compile error is printed as JSON on stdout
    const natErr = natOut.startsWith("{") ? JSON.parse(natOut) : null;
    const codes = (j) => (j && j.errors || []).map((x) => x.code).join(",");
    if (natErr) {
      if (codes(natErr) !== codes(got)) fail(`play/${e.id}`, `errors ${codes(got)}, native ${codes(natErr)}`);
      else { n++; console.log(`  ${e.id}: compile error ${codes(got)} as native`); }
    } else if (got.stdout !== natOut || got.exit !== nat.status) {
      fail(`play/${e.id}`, `differs from native (exit ${got.exit} vs ${nat.status})\n--- native\n${natOut}${nat.stderr}--- wasm\n${got.stdout}${JSON.stringify(got.errors || "")}`);
    } else {
      n++;
      console.log(`  ${e.id}: ${got.stdout.split("\n").length - 1} lines, same as native${got.fixed ? `, ${got.fixed.length} fix(es) in memory` : ""}`);
    }
    // every example also checks, formats and (when it has errors) fixes without trapping
    for (const f of ["check", "fmt", "fix"]) JSON.parse(call(f, e.code));
  }
  fs.rmSync(tmp, { recursive: true, force: true });
  console.log(`playground: ${n}/${EXAMPLES.length} as native`);
  passed += n;
}

// ---- 3. the other exports, and the safety limits ----
const checks = [
  ["check finds an error", () => JSON.parse(call("check", "let x = 1\nprint(y)\n")).errors[0].code === "E0201"],
  ["compile_js gives JavaScript", () => !call("compile_js", "print(1)\n").startsWith("{")],
  ["explain gives the text", () => JSON.parse(call("explain", "E0290")).text.includes("What it means")],
  ["fmt indents", () => JSON.parse(call("fmt", "fn f() -> int {\nreturn 1\n}\nprint(f())\n")).source.includes("    return 1")],
  ["fuel stops a loop", () => run("var i = 0\nwhile true {\n    i += 1\n}\n", "", "fuel=100000").errors[0].code === "E0355"],
  ["time stops a loop", () => run("var i = 0\nwhile true {\n    i += 1\n}\n", "", "time=300").errors[0].code === "E0359"],
  ["depth stops recursion", () => run("fn f(n: int) -> int = f(n + 1)\nprint(f(0))\n").errors[0].code === "E0358"],
  ["fs is not granted", () => run("use fs\nprint(fs.exists(\"x\"))\n").errors[0].code === "E0290"],
  ["os is not granted", () => run("use os\nprint(os.args())\n").errors[0].code === "E0290"],
  ["the instance survives", () => run("print(\"still here\")\n").stdout === "still here\n"],
];
for (const [what, ok] of checks) {
  let r = false;
  try { r = ok(); } catch (e) { r = false; console.log(`  ${what}: threw ${e.message}`); }
  if (r) passed++; else fail(what, "no");
}
console.log(`${passed} passed, ${failed} failed`);
process.exit(failed ? 1 : 0);
