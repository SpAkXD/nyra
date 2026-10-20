// The WebAssembly backend: the whole compiler and its sandboxed interpreter as nyra.wasm (built by
// tools/build_wasm.py from src/wasm.rs), so no executable is needed: Node.js, Deno, Bun, browsers
// and Cloudflare Workers. It grants only the capability `input` (no files, OS or network).
//
// The C ABI (the same as the playground's site/play/nyra.js): strings go in through buffers from
// `nyra_alloc` and come back as (pointer, `result_len()`), valid until the next call.
import { loadBytes } from "#load";
import { caps, diag, finish, needsNative } from "./common.js";

const enc = new TextEncoder();
const dec = new TextDecoder();

/** The default nyra.wasm: next to this package (copied there by scripts/build-wasm.mjs). */
export const DEFAULT_WASM = new URL("../wasm/nyra.wasm", import.meta.url);

const modules = new Map(); // compiled once per source

/** A WebAssembly.Module from a Module, bytes, a Response (or a promise of one), or a URL. */
export async function compileNyra(source = DEFAULT_WASM) {
  source = await source;
  if (source instanceof WebAssembly.Module) return source;
  const key = source instanceof URL || typeof source === "string" ? String(source) : null;
  if (key && modules.has(key)) return modules.get(key);
  const job = (async () => {
    let s = source;
    if (key) s = await loadBytes(new URL(key, import.meta.url));
    if (typeof Response !== "undefined" && s instanceof Response) {
      if (!s.ok) throw new Error(`cannot load nyra.wasm: HTTP ${s.status}`);
      if (WebAssembly.compileStreaming && (s.headers.get("content-type") || "").startsWith("application/wasm")) {
        try { return await WebAssembly.compileStreaming(s.clone()); } catch (_) { /* read the bytes below */ }
      }
      s = await s.arrayBuffer();
    }
    return WebAssembly.compile(s);
  })();
  if (key) {
    modules.set(key, job);
    job.catch(() => modules.delete(key));
  }
  return job;
}

/** One instance of the compiler: cheap to make, made again after a trap. */
export async function instantiateNyra(module) {
  const env = {
    nyra_now_ms: () => Date.now(),
    nyra_mono_ms: () => performance.now(),
    nyra_random: () => Math.random(),
  };
  const ex = (await WebAssembly.instantiate(module, { env })).exports;
  const take = (ptr) => dec.decode(new Uint8Array(ex.memory.buffer, ptr, ex.result_len()).slice());
  function call(fn, ...texts) {
    const bufs = texts.map((t) => {
      const bytes = enc.encode(t);
      const ptr = ex.nyra_alloc(bytes.length);
      new Uint8Array(ex.memory.buffer, ptr, bytes.length).set(bytes);
      return [ptr, bytes.length];
    });
    // the memory may grow during the call: `take` makes a fresh view
    const out = take(fn(...bufs.flat()));
    for (const [p, n] of bufs) ex.nyra_free(p, n);
    return out;
  }
  const json = (fn, ...texts) => JSON.parse(call(fn, ...texts));
  return {
    version: () => take(ex.version()),
    check: (src) => json(ex.check, src),
    js: (src) => {
      const out = call(ex.compile_js, src);
      return out.startsWith('{"ok":false') ? JSON.parse(out) : { ok: true, js: out };
    },
    run: (src, stdin = "", limits = "") => json(ex.run_interp, src, stdin, limits),
    explain: (code) => json(ex.explain, code),
    fmt: (src) => json(ex.fmt, src),
    fix: (src) => json(ex.fix, src),
  };
}

const STACK = /call stack|too much recursion|stack overflow/i;

export class WasmBackend {
  constructor(opts) {
    if (needsNative(opts.allow)) {
      throw new Error(`the WebAssembly backend grants only \`input\`, not ${caps(opts.allow)}: use the native backend (the nyra executable) for files or the OS`);
    }
    if (opts.cwd) throw new Error("`cwd` needs the native backend (the WebAssembly backend has no files)");
    if (opts.native) throw new Error("`native: true` needs the native backend");
    this.opts = opts;
    this.name = "wasm";
    this.module = null;
    this.nyra = null;
  }

  async ready() {
    if (!this.nyra) {
      this.module ??= await compileNyra(this.opts.wasm ?? DEFAULT_WASM).catch((e) => {
        throw new Error(`cannot load nyra.wasm (${e.message}); pass it as \`wasm\` (a WebAssembly.Module, bytes or a URL), e.g. in Cloudflare Workers: import wasm from "@nyralang/nyra/nyra.wasm"`);
      });
      this.nyra = await instantiateNyra(this.module);
    }
    return this.nyra;
  }

  /** Calls the compiler; a trap leaves the instance in an unknown state, so the next call makes a new one. */
  async guarded(fn) {
    const nyra = await this.ready();
    try {
      return { value: fn(nyra) };
    } catch (e) {
      this.nyra = null;
      if (e instanceof WebAssembly.RuntimeError || STACK.test(String(e && e.message))) return { trap: e };
      throw e;
    }
  }

  limits(timeoutMs = this.opts.timeoutMs) {
    const o = this.opts;
    const parts = [`fuel=${o.fuel}`, `memory=${Math.max(1, Math.floor(o.memoryMb))}M`, `output=${o.maxOutput}`, `time=${Math.max(1, Math.floor(timeoutMs))}`];
    if (o.maxDepth != null) parts.push(`depth=${o.maxDepth}`);
    if (o.strict) parts.push("strict=1");
    return parts.join(" ");
  }

  async run(code, stdin = "", { args, files } = {}) {
    if (args && args.length) throw new Error("`args` needs the native backend (the WebAssembly backend grants no `os`)");
    if (files && Object.keys(files).length) throw new Error("`files` (modules next to the program) need the native backend");
    const start = performance.now();
    const r = await this.guarded((nyra) => nyra.run(code, stdin, this.limits()));
    const ms = Math.round((performance.now() - start) * 10) / 10;
    if (r.trap) {
      // deeper recursion than the engine's own stack allows
      const e = diag({ code: "E0358", message: `the program stopped: ${r.trap.message}`, hint: "a recursion too deep for the WebAssembly engine's stack; lower `maxDepth` or use the native backend", runtime: true });
      return finish({ exitCode: 123, errors: [e], ms, backend: this.name });
    }
    const j = r.value;
    const errors = (j.errors || []).map(diag);
    const warnings = [...(j.warnings || []), ...(j.fixed || [])].map(diag);
    return finish({
      stdout: j.stdout ?? "",
      stderr: j.stderr ?? "",
      exitCode: j.exit,
      errors,
      warnings,
      steps: j.steps ?? null,
      ms,
      stage: j.stage === "compile" ? "compile" : null,
      backend: this.name,
    });
  }

  /** Always strict: errors with a certain fix are errors (their `fix` says how). */
  async check(code, { files } = {}) {
    if (files && Object.keys(files).length) throw new Error("`files` need the native backend");
    const start = performance.now();
    const r = await this.guarded((nyra) => nyra.check(code));
    if (r.trap) throw r.trap;
    const j = r.value;
    return { ok: !!j.ok, errors: (j.errors || []).map(diag), warnings: (j.warnings || []).map(diag), ms: Math.round((performance.now() - start) * 10) / 10 };
  }

  /** Every certain fix is applied, also when errors without one remain. */
  async fix(code) {
    const r = await this.guarded((nyra) => nyra.fix(code));
    if (r.trap) throw r.trap;
    return r.value.source ?? code;
  }

  async explain(code) {
    const r = await this.guarded((nyra) => nyra.explain(code));
    if (r.trap) throw r.trap;
    return r.value;
  }

  async version() {
    return (await this.ready()).version();
  }
}
