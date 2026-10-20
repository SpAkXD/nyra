// The native backend (Node.js, Deno, Bun): the `nyra` executable in a child process, always with
// `--sandbox` unless `native: true` is asked for. Loaded through the `#native` import, which maps
// to native-stub.js outside Node, so browser and Workers bundles never see `node:` modules.
import { spawn } from "node:child_process";
import { existsSync, statSync } from "node:fs";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { FILE, caps, diag, finish } from "./common.js";

export const available = true;

const EXE = process.platform === "win32" ? "nyra.exe" : "nyra";
const TIME_LINE = /^nyra .*\| interpreted ([\d.]+) ms \((\d+) steps/;

function executable(p) {
  try {
    const s = statSync(p);
    if (s.isDirectory()) return executable(path.join(p, EXE));
    return s.isFile() ? p : null;
  } catch (_) {
    return null;
  }
}

/** The `nyra` executable: `binary`, NYRA_PATH (the file or its folder), then PATH; or null. */
export function findBinary(binary) {
  if (binary) return executable(binary);
  const env = process.env.NYRA_PATH;
  if (env) return executable(env);
  const exts = process.platform === "win32" ? (process.env.PATHEXT || ".EXE").split(";").map((e) => e.toLowerCase()) : [""];
  for (const dir of (process.env.PATH || "").split(path.delimiter).filter(Boolean)) {
    for (const ext of exts) {
      const p = path.join(dir, "nyra" + ext);
      if (existsSync(p) && executable(p)) return p;
    }
  }
  return null;
}

function exec(bin, args, { stdin = "", cwd, timeoutMs }) {
  return new Promise((resolve, reject) => {
    const child = spawn(bin, args, { cwd, env: { ...process.env, NO_COLOR: "1" }, windowsHide: true });
    const out = [], err = [];
    let killed = false;
    const timer = setTimeout(() => { killed = true; child.kill("SIGKILL"); }, timeoutMs);
    child.stdout.on("data", (b) => out.push(b));
    child.stderr.on("data", (b) => err.push(b));
    child.on("error", (e) => { clearTimeout(timer); reject(e); });
    child.on("close", (code) => {
      clearTimeout(timer);
      resolve({ code: code ?? -1, killed, stdout: Buffer.concat(out).toString("utf8"), stderr: Buffer.concat(err).toString("utf8") });
    });
    child.stdin.on("error", () => {}); // a program that exits without reading its input
    child.stdin.end(stdin, "utf8");
  });
}

async function withProgram(code, files, fn) {
  const tmp = await mkdtemp(path.join(os.tmpdir(), "nyra-"));
  try {
    for (const [name, text] of Object.entries(files || {})) {
      const p = path.resolve(tmp, name);
      if (!p.startsWith(path.resolve(tmp) + path.sep) || name === FILE) {
        throw new TypeError(`file name ${JSON.stringify(name)} must be a relative path inside the program's folder, not ${FILE}`);
      }
      await mkdir(path.dirname(p), { recursive: true });
      await writeFile(p, text, "utf8");
    }
    await writeFile(path.join(tmp, FILE), code, "utf8");
    return await fn(tmp);
  } finally {
    await rm(tmp, { recursive: true, force: true });
  }
}

export class NativeBackend {
  constructor(opts, bin) {
    this.opts = opts;
    this.bin = bin;
    this.name = opts.native ? "native-compiled" : "native";
  }

  async run(code, stdin = "", { args, files } = {}) {
    const o = this.opts;
    const start = performance.now();
    return withProgram(code, files, async (tmp) => {
      const cwd = o.cwd ?? tmp;
      const cmd = ["run", o.cwd ? path.join(tmp, FILE) : FILE, "--json", "--allow", caps(o.allow)];
      if (o.native) cmd.push("--native");
      else {
        cmd.push("--sandbox", "--time", "--fuel", String(o.fuel), "--max-memory", `${Math.max(1, Math.floor(o.memoryMb))}M`,
          "--max-output", String(o.maxOutput), "--max-time", String(Math.max(1, Math.floor(o.timeoutMs))));
        if (o.maxDepth != null) cmd.push("--max-depth", String(o.maxDepth));
      }
      if (o.strict) cmd.push("--strict");
      if (args && args.length) cmd.push("--", ...args.map(String));
      // the sandbox stops itself at the time limit; the process gets a margin before it is killed
      const hard = o.timeoutMs + (o.native ? 30_000 : 5_000);
      const p = await exec(this.bin, cmd, { stdin, cwd, timeoutMs: hard });
      const ms = Math.round((performance.now() - start) * 10) / 10;
      if (p.killed) {
        const e = diag({ code: "E0359", message: `time limit reached: the program was killed after ${hard} ms`, hint: "an endless loop? Otherwise raise `timeoutMs`", runtime: true });
        return finish({ stdout: p.stdout, exitCode: 124, errors: [e], ms, backend: this.name });
      }
      return parse(p, ms, this.name);
    });
  }

  async check(code, { strict = true, files } = {}) {
    const start = performance.now();
    return withProgram(code, files, async (tmp) => {
      const cmd = ["check", FILE, "--json", "--allow", caps(this.opts.allow)];
      if (strict) cmd.push("--strict");
      const p = await exec(this.bin, cmd, { cwd: tmp, timeoutMs: this.opts.timeoutMs + 30_000 });
      let j;
      try {
        j = JSON.parse(p.stdout);
      } catch (_) {
        throw new Error(`nyra check printed no JSON: ${p.stderr.trim()}`);
      }
      return { ok: !!j.ok, errors: (j.errors || []).map(diag), warnings: (j.warnings || []).map(diag), ms: Math.round((performance.now() - start) * 10) / 10 };
    });
  }

  async fix(code) {
    return withProgram(code, null, async (tmp) => {
      await exec(this.bin, ["check", FILE, "--fix", "--json", "--allow", caps(this.opts.allow)], { cwd: tmp, timeoutMs: this.opts.timeoutMs + 30_000 });
      return readFile(path.join(tmp, FILE), "utf8");
    });
  }

  async explain(code) {
    const cwd = os.tmpdir();
    const p = await exec(this.bin, ["explain", code, "--json"], { cwd, timeoutMs: 30_000 });
    const j = JSON.parse(p.stdout);
    if (p.code === 0 && j && j.code) {
      j.ok ??= true;
      j.text = (await exec(this.bin, ["explain", code], { cwd, timeoutMs: 30_000 })).stdout;
    }
    return j;
  }

  async version() {
    const p = await exec(this.bin, ["--version"], { cwd: os.tmpdir(), timeoutMs: 30_000 });
    return (p.stdout || p.stderr).trim().split(/\s+/).pop();
  }
}

function parse(p, ms, backend) {
  let stdout = p.stdout;
  let errors = [], warnings = [], steps = null, stage = null;
  const rest = [];
  for (const line of p.stderr.split(/\r?\n/)) {
    if (!line) continue;
    const m = TIME_LINE.exec(line);
    if (m) { steps = Number(m[2]); continue; }
    if (line.startsWith("{")) {
      let j = null;
      try { j = JSON.parse(line); } catch (_) { /* text */ }
      if (j && Array.isArray(j.errors)) { errors.push(...j.errors.map(diag)); continue; }
      if (j && Array.isArray(j.warnings)) { warnings.push(...j.warnings.map(diag)); continue; }
    }
    rest.push(line);
  }
  // a compile error: the errors as JSON on standard output, exit 1, and nothing ran
  if (p.code === 1 && steps === null && !errors.length && stdout.startsWith('{"ok":false')) {
    try {
      const j = JSON.parse(stdout);
      errors = (j.errors || []).map(diag);
      warnings.push(...(j.warnings || []).map(diag));
      stdout = "";
      stage = "compile";
    } catch (_) { /* the program's own output */ }
  }
  return finish({ stdout, stderr: rest.join("\n"), exitCode: p.code, errors, warnings, steps, ms, stage, backend });
}
