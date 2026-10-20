// Puts the WebAssembly compiler into this package (wasm/nyra.wasm, not committed: it is built from
// the compiler's sources like site/play/nyra.wasm is).
//
//   node scripts/build-wasm.mjs            copy target/wasm32-unknown-unknown/wasm/nyra.wasm
//   node scripts/build-wasm.mjs --build    build it first (python tools/build_wasm.py --no-copy)
//   node scripts/build-wasm.mjs --from F   copy another build
//   node scripts/build-wasm.mjs --check    fail unless wasm/nyra.wasm is there (npm pack runs this)
import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const pkg = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const repo = path.resolve(pkg, "..", "..");
const dest = path.join(pkg, "wasm", "nyra.wasm");
const argv = process.argv.slice(2);
const from = argv.includes("--from") ? path.resolve(argv[argv.indexOf("--from") + 1]) : path.join(repo, "target", "wasm32-unknown-unknown", "wasm", "nyra.wasm");

if (argv.includes("--check")) {
  if (!existsSync(dest)) {
    console.error("wasm/nyra.wasm is missing: run `npm run build:wasm` (needs Rust and `rustup target add wasm32-unknown-unknown`)");
    process.exit(1);
  }
  process.exit(0);
}

if (argv.includes("--build")) {
  const py = process.platform === "win32" ? "python" : "python3";
  const r = spawnSync(py, [path.join(repo, "tools", "build_wasm.py"), "--no-copy"], { cwd: repo, stdio: "inherit", env: { CARGO_INCREMENTAL: "0", ...process.env } });
  if (r.status !== 0) process.exit(r.status ?? 1);
}

if (!existsSync(from)) {
  console.error(`${from} does not exist: build it with \`python tools/build_wasm.py --no-copy\` (or run this with --build)`);
  process.exit(1);
}
mkdirSync(path.dirname(dest), { recursive: true });
copyFileSync(from, dest);
console.log(`build-wasm: ${path.relative(pkg, dest)}: ${statSync(dest).size.toLocaleString("en")} bytes`);
