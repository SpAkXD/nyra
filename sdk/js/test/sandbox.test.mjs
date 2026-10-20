// node --test test/   (the native backend uses target/release/nyra of this repository, or NYRA_PATH;
// the WebAssembly backend uses wasm/nyra.wasm from scripts/build-wasm.mjs)
import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { describe, it } from "node:test";
import { fileURLToPath } from "node:url";

const pkg = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const exe = path.resolve(pkg, "../../target/release", process.platform === "win32" ? "nyra.exe" : "nyra");
if (!process.env.NYRA_PATH && existsSync(exe)) process.env.NYRA_PATH = exe;

const nyra = await import("../src/index.js");
const { Sandbox, NyraError } = nyra;

const HAVE_NATIVE = !!nyra.findBinary();
const HAVE_WASM = existsSync(path.join(pkg, "wasm", "nyra.wasm"));
const LOOP = "var i = 0\nwhile true {\n    i += 1\n}\n";
const FIXABLE = "let xs = [1, 2]\nxs.append(3)\nprint(len(xs));\n";

for (const backend of ["native", "wasm"]) {
  const skip = backend === "native" ? (!HAVE_NATIVE && "no nyra executable (cargo build --release)") : (!HAVE_WASM && "no wasm/nyra.wasm (npm run build:wasm)");
  const box = (o = {}) => new Sandbox({ backend, ...o });

  describe(`${backend} backend`, { skip }, () => {
    it("prints and reads stdin", async () => {
      const r = await box().run('use input\nlet name = input.line()\nprint("hi {name}")\n', "Ada\n");
      assert.equal(r.stdout, "hi Ada\n", r.stderr);
      assert.equal(r.ok, true);
      assert.equal(r.exitCode, 0);
      assert.deepEqual(r.errors, []);
      assert.equal(r.stage, null);
      assert.ok(r.steps > 0);
      assert.equal(r.backend, backend);
    });

    it("keeps unicode", async () => {
      const r = await box().run("use input\nprint(input.all())\n", "žluťoučký kůň 🐎");
      assert.equal(r.stdout, "žluťoučký kůň 🐎\n");
    });

    it("structures compile errors", async () => {
      const r = await box().run("let x = 1\nprint(y)\n");
      assert.equal(r.ok, false);
      assert.equal(r.stage, "compile");
      assert.equal(r.exitCode, 1);
      const e = r.errors[0];
      assert.deepEqual([e.code, e.line, e.col], ["E0201", 2, 7]);
      assert.ok(e.hint);
      assert.equal(e.file, "main.nyra");
      assert.match(r.stderr, /E0201/);
      assert.match(r.feedback(), /did not compile/);
    });

    it("structures runtime errors", async () => {
      const r = await box().run("let xs = [1, 2]\nprint(xs[5])\n");
      assert.equal(r.stage, "runtime");
      assert.equal(r.exitCode, 101);
      assert.equal(r.errors[0].code, "E0240");
      assert.equal(r.errors[0].line, 2);
      assert.equal(r.limit, null);
    });

    it("stops at the fuel limit", async () => {
      const r = await box({ fuel: 100_000 }).run(LOOP);
      assert.deepEqual([r.exitCode, r.limit, r.errors[0].code], [120, "fuel", "E0355"]);
    });

    it("stops at the time limit", async () => {
      const r = await box({ fuel: 1e15, timeoutMs: 300 }).run(LOOP);
      assert.deepEqual([r.exitCode, r.limit], [124, "time"]);
      assert.ok(r.ms < 5000, `${r.ms} ms`);
    });

    it("stops at the memory limit", async () => {
      const r = await box({ memoryMb: 8 }).run('var s = "x"\nwhile true {\n    s = s + s\n}\n');
      assert.deepEqual([r.exitCode, r.limit], [121, "memory"], r.stderr);
    });

    it("stops at the output limit", async () => {
      const r = await box({ maxOutput: 100 }).run('for i in 0..1000 {\n    print("line {i}")\n}\n');
      assert.deepEqual([r.exitCode, r.limit], [122, "output"], r.stderr);
      assert.ok(Buffer.byteLength(r.stdout) <= 100);
    });

    it("stops at the depth limit", async () => {
      const r = await box({ maxDepth: 50 }).run("fn f(n: int) -> int = f(n + 1)\nprint(f(0))\n");
      assert.deepEqual([r.exitCode, r.limit], [123, "depth"], r.stderr);
    });

    it("grants no capability but input", async () => {
      for (const m of ["fs", "os"]) {
        const r = await box().run(`use ${m}\nprint(1)\n`);
        assert.equal(r.stage, "compile");
        assert.equal(r.errors[0].code, "E0290");
      }
    });

    it("repairs certain errors in memory unless strict", async () => {
      const r = await box().run(FIXABLE);
      assert.equal(r.stdout, "3\n", r.stderr);
      assert.ok(r.warnings.some((w) => w.applied));
      assert.match(r.feedback(), /repaired in memory/);
      assert.equal((await box({ strict: true }).run(FIXABLE)).stage, "compile");
    });

    it("checks", async () => {
      assert.equal((await box().check('print("ok")\n')).ok, true);
      const c = await box().check(FIXABLE);
      assert.equal(c.ok, false);
      assert.ok(c.errors[0].fix);
    });

    it("fixes", async () => {
      assert.equal(await box().fix(FIXABLE), "var xs = [1, 2]\nxs.push(3)\nprint(xs.len())\n");
    });

    it("explains", async () => {
      const e = await box().explain("E0201");
      assert.equal(e.code, "E0201");
      assert.equal(e.ok, true);
      assert.match(e.text, /E0201/);
      assert.equal((await box().explain("E9999")).ok, false);
    });

    it("parses JSON output", async () => {
      const v = await box().runJson('use json\nstruct P { x: int, tags: [str] }\nprint(json.str(P(x: 1, tags: ["a"])))\n');
      assert.deepEqual(v, { x: 1, tags: ["a"] });
      await assert.rejects(box().runJson("print(nope)\n"), (e) => e instanceof NyraError && e.result.errors[0].code === "E0201");
      await assert.rejects(box().runJson('print("not json")\n'), NyraError);
    });

    it("tells its version", async () => {
      assert.match(await box().version(), /^\d+\.\d+\.\d+/);
    });
  });
}

describe("native only", { skip: !HAVE_NATIVE && "no nyra executable" }, () => {
  it("is the default when an executable is found", () => {
    assert.equal(new Sandbox().backendName, "native");
  });

  it("confines files to cwd", async () => {
    const d = mkdtempSync(path.join(os.tmpdir(), "nyra-js-test-"));
    try {
      const box = new Sandbox({ allow: ["fs"], cwd: d });
      const r = await box.run('use fs\nfs.write("note.txt", "kept")\nprint(fs.read("note.txt"))\n');
      assert.equal(r.stdout, "kept\n", r.stderr);
      assert.equal(readFileSync(path.join(d, "note.txt"), "utf8"), "kept");
      const bad = await box.run('use fs\nfs.write("../escaped-nyra.txt", "no")\n');
      assert.equal(bad.ok, false);
      assert.equal(existsSync(path.join(d, "..", "escaped-nyra.txt")), false);
    } finally {
      rmSync(d, { recursive: true, force: true });
    }
  });

  it("passes args and modules", async () => {
    const r = await new Sandbox({ allow: "os" }).run("use os\nprint(os.args())\n", "", { args: ["a", "b c"] });
    assert.equal(r.stdout, '["a", "b c"]\n', r.stderr);
    const files = { "shapes.nyra": "pub struct Rect { w: int, h: int }\npub fn area(r: Rect) -> int = r.w * r.h\n" };
    const m = await new Sandbox().run("use ./shapes\nprint(shapes.area(Rect(w: 3, h: 4)))\n", "", { files });
    assert.equal(m.stdout, "12\n", m.stderr);
    await assert.rejects(new Sandbox().run("print(1)\n", "", { files: { "../evil.nyra": "" } }), TypeError);
  });

  it("removes its temporary folders", async () => {
    const before = new Set(readdirSync(os.tmpdir()).filter((n) => n.startsWith("nyra-")));
    await Promise.all([1, 2, 3].map((i) => new Sandbox().run(`print(${i})\n`)));
    const left = readdirSync(os.tmpdir()).filter((n) => n.startsWith("nyra-") && !before.has(n));
    // another process may make one at the same moment; ours are gone
    assert.ok(left.length === 0, `left: ${left}`);
  });
});

describe("wasm only", { skip: !HAVE_WASM && "no wasm/nyra.wasm" }, () => {
  it("refuses capabilities it cannot grant", () => {
    assert.throws(() => new Sandbox({ backend: "wasm", allow: ["fs"] }).backendName, /grants only `input`/);
    assert.throws(() => new Sandbox({ backend: "wasm", cwd: "." }).backendName, /native backend/);
  });

  it("works without an executable", async () => {
    const saved = { p: process.env.PATH, n: process.env.NYRA_PATH };
    process.env.PATH = "";
    delete process.env.NYRA_PATH;
    try {
      const box = new Sandbox();
      assert.equal(box.backendName, "wasm");
      assert.equal((await box.run("print(6 * 7)\n")).stdout, "42\n");
      assert.throws(() => new Sandbox({ allow: ["fs"] }).backendName, /no nyra executable/);
    } finally {
      process.env.PATH = saved.p;
      if (saved.n) process.env.NYRA_PATH = saved.n;
    }
  });

  it("applies every certain fix even when an error remains", async () => {
    const src = "let xs = [1]\nxs.append(2)\nprint(cout)\n";
    assert.equal(await new Sandbox({ backend: "wasm" }).fix(src), "var xs = [1]\nxs.push(2)\nprint(cout)\n");
  });

  it("survives a trap", async () => {
    const box = new Sandbox({ backend: "wasm", maxDepth: 20_000 });
    const r = await box.run("fn f(n: int) -> int = f(n + 1)\nprint(f(0))\n");
    assert.equal(r.ok, false);
    assert.equal(r.limit, "depth");
    assert.equal((await box.run('print("still here")\n')).stdout, "still here\n");
  });

  it("accepts a compiled module", async () => {
    const mod = await nyra.compileNyra(readFileSync(path.join(pkg, "wasm", "nyra.wasm")));
    const r = await new Sandbox({ backend: "wasm", wasm: mod }).run("print(1)\n");
    assert.equal(r.stdout, "1\n");
  });
});

describe("tools", () => {
  it("has specs for the model APIs", () => {
    const a = nyra.anthropicToolSpec();
    assert.equal(a.name, "run_nyra");
    assert.deepEqual(a.input_schema.required, ["code"]);
    const o = nyra.openaiToolSpec();
    assert.equal(o.function.name, "run_nyra");
    a.input_schema.extra = 1;
    assert.equal(nyra.PARAMETERS.extra, undefined);
  });

  it("runs a tool call", { skip: !(HAVE_NATIVE || HAVE_WASM) && "no backend" }, async () => {
    const text = await nyra.runTool({ code: "use input\nprint(input.line())\n", stdin: "echo\n" });
    assert.match(text, /echo/);
    assert.match(await nyra.runTool({ code: "print(nope)\n" }), /E0201/);
  });
});
