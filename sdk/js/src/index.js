// @nyralang/nyra: run AI-written Nyra code safely, with the `nyra` executable (Node.js) or the
// bundled WebAssembly compiler (anywhere). See index.d.ts for the API and README.md for examples.
import * as native from "#native";
import { CAPABILITIES, NyraError, Result, needsNative, options, parseJsonOutput } from "./common.js";
import { WasmBackend, compileNyra, instantiateNyra } from "./wasm.js";

export { CAPABILITIES, NyraError, Result, compileNyra, instantiateNyra };
export { TOOL_NAME, TOOL_DESCRIPTION, PARAMETERS, anthropicToolSpec, openaiToolSpec, runTool } from "./tools.js";

/** The `nyra` executable this process would use (Node.js only), or null. */
export function findBinary(binary) {
  try {
    return native.findBinary(binary);
  } catch (_) {
    return null; // no permission to read the environment or the disk (Deno), or not Node.js
  }
}

export class Sandbox {
  /** @param {import("./index.d.ts").SandboxOptions} [opts] */
  constructor(opts = {}) {
    this.options = options(opts);
    this._backend = null;
  }

  /** Picks the backend on first use: the executable when there is one (or it is needed), else WebAssembly. */
  backend() {
    if (this._backend) return this._backend;
    const o = this.options;
    let kind = o.backend;
    let bin = null;
    if (kind !== "wasm") {
      bin = native.available ? findBinary(o.binary) : null;
      if (kind === "native" || o.binary || o.native || o.cwd || needsNative(o.allow)) {
        if (!bin) {
          const why = o.binary ? `no nyra executable at ${o.binary}` : "no nyra executable found (NYRA_PATH, PATH)";
          const need = kind === "native" ? "" : " (needed for `allow`, `cwd`, `native` or `binary`)";
          throw new Error(`${why}${need}; install it from https://nyralang.dev/install or use backend: "wasm"`);
        }
        kind = "native";
      } else {
        kind = bin ? "native" : "wasm";
      }
    }
    this._backend = kind === "native" ? new native.NativeBackend(o, bin) : new WasmBackend(o);
    return this._backend;
  }

  /** "native", "native-compiled" or "wasm". */
  get backendName() {
    return this.backend().name;
  }

  /** Compiles and runs `code` with `stdin` as its standard input. */
  run(code, stdin = "", extra = {}) {
    return this.backend().run(code, stdin, extra);
  }

  /** Errors and warnings without running (`strict` by default: fixable errors are errors). */
  check(code, extra = {}) {
    return this.backend().check(code, extra);
  }

  /** `code` with the certain fixes of its errors applied. */
  fix(code) {
    return this.backend().fix(code);
  }

  /** The entry of an error code with its text in `text`; `{ok: false, ...}` for an unknown code. */
  explain(code) {
    return this.backend().explain(code);
  }

  /** Runs `code` and parses what it printed as one JSON value; throws NyraError when it fails. */
  async runJson(code, stdin = "", extra = {}) {
    return parseJsonOutput(await this.run(code, stdin, extra));
  }

  version() {
    return this.backend().version();
  }
}

let shared = null;
const box = (opts) => (opts ? new Sandbox(opts) : (shared ??= new Sandbox()));

export const run = (code, stdin = "", opts) => box(opts).run(code, stdin);
export const check = (code, opts) => box(opts).check(code);
export const fix = (code, opts) => box(opts).fix(code);
export const explain = (code, opts) => box(opts).explain(code);
export const runJson = (code, stdin = "", opts) => box(opts).runJson(code, stdin);
