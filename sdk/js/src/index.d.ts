// Types of @nyralang/nyra.

export type Capability = "fs" | "input" | "os" | "net" | "all";
export type BackendName = "native" | "native-compiled" | "wasm";
export type Limit = "fuel" | "memory" | "output" | "depth" | "time";

/** One edit of a certain fix (1-based lines and columns, the end exclusive). */
export interface FixEdit {
  line: number;
  col: number;
  end_line: number;
  end_col: number;
  text: string;
}

/** A compile or runtime error, or a warning. */
export interface Diagnostic {
  /** `E0201`; `nyra explain` / `explain()` describes it. */
  code: string;
  message: string;
  line: number;
  col: number;
  /** What to do about it ("" when there is nothing to add). */
  hint: string;
  /** The edits of a certain fix, or null. */
  fix: FixEdit[] | null;
  file?: string;
  /** A runtime error (not a compile error). */
  runtime?: boolean;
  /** For a fix applied in memory: the line as it was repaired. */
  applied?: string;
  [key: string]: unknown;
}

export declare class Result {
  /** It compiled and ran to the end with exit code 0. */
  ok: boolean;
  stdout: string;
  /** The errors as text, and anything else the program or the compiler wrote there. */
  stderr: string;
  /** 0; 1 for a compile error; 101 for a runtime error; 120-124 for the limits (fuel, memory, output, depth, time). */
  exitCode: number;
  errors: Diagnostic[];
  /** Warnings, and the fixes applied in memory (with `applied`) unless `strict`. */
  warnings: Diagnostic[];
  /** Interpreter steps (null with `native: true`). */
  steps: number | null;
  /** Wall time of the call, in milliseconds. */
  ms: number;
  stage: "compile" | "runtime" | null;
  /** The limit that stopped the program. */
  limit: Limit | null;
  backend: BackendName;
  /** A short text for a model: the output, then each error with its line and hint. */
  feedback(maxChars?: number): string;
}

export interface CheckResult {
  ok: boolean;
  errors: Diagnostic[];
  warnings: Diagnostic[];
  ms: number;
}

export interface SandboxOptions {
  /** Steps a run may take (E0355). Default 100,000,000. */
  fuel?: number;
  /** Real time a run may take, in ms (E0359). Default 10,000. */
  timeoutMs?: number;
  /** Heap a run may use, in MiB (E0356). Default 256. */
  memoryMb?: number;
  /** Bytes a run may print (E0357). Default 1 MiB. */
  maxOutput?: number;
  /** Nested calls (E0358). Default: the interpreter's. */
  maxDepth?: number;
  /** Capabilities besides `input` (native backend only): "fs", "os", "net" or "all". */
  allow?: Capability[] | string;
  /** The working folder (native backend); with `allow: ["fs"]` files stay below it. Default: a fresh temporary folder per run. */
  cwd?: string;
  /** Do not repair errors that have one certain fix in memory. */
  strict?: boolean;
  /** Compile with the C compiler and run natively: NO sandbox (only `timeoutMs` applies). Trusted code only. */
  native?: boolean;
  /** "auto" (default): the `nyra` executable when one is found, else WebAssembly. */
  backend?: "auto" | "native" | "wasm";
  /** The `nyra` executable (default: NYRA_PATH, then PATH). */
  binary?: string;
  /** nyra.wasm: a WebAssembly.Module (Cloudflare Workers: `import wasm from "@nyralang/nyra/nyra.wasm"`), bytes, a Response or a URL. Default: the package's own. */
  wasm?: WebAssembly.Module | ArrayBuffer | Uint8Array | Response | Promise<Response> | URL | string;
}

export interface RunExtra {
  /** Arguments for `os.args()` (native backend, `allow: ["os"]`). */
  args?: string[];
  /** More modules next to main.nyra, for `use ./name` (native backend). */
  files?: Record<string, string>;
}

export declare class Sandbox {
  constructor(options?: SandboxOptions);
  readonly options: Required<Omit<SandboxOptions, "allow">> & { allow: Capability[] };
  /** The backend this sandbox uses ("native", "native-compiled" or "wasm"). */
  readonly backendName: BackendName;
  run(code: string, stdin?: string, extra?: RunExtra): Promise<Result>;
  /** Strict by default (fixable errors are errors); the WebAssembly backend is always strict. */
  check(code: string, extra?: { strict?: boolean; files?: Record<string, string> }): Promise<CheckResult>;
  /** The certain fixes applied. Native: only when they make it compile; WebAssembly: every one. */
  fix(code: string): Promise<string>;
  explain(code: string): Promise<Record<string, unknown> & { ok: boolean; text?: string }>;
  /** Runs `code` and parses what it printed as one JSON value; throws NyraError when it fails. */
  runJson<T = unknown>(code: string, stdin?: string, extra?: RunExtra): Promise<T>;
  version(): Promise<string>;
}

export declare class NyraError extends Error {
  result: Result;
}

export declare const CAPABILITIES: readonly Capability[];
export declare function findBinary(binary?: string): string | null;

export declare function run(code: string, stdin?: string, options?: SandboxOptions): Promise<Result>;
export declare function check(code: string, options?: SandboxOptions): Promise<CheckResult>;
export declare function fix(code: string, options?: SandboxOptions): Promise<string>;
export declare function explain(code: string, options?: SandboxOptions): Promise<Record<string, unknown> & { ok: boolean; text?: string }>;
export declare function runJson<T = unknown>(code: string, stdin?: string, options?: SandboxOptions): Promise<T>;

// tools for model APIs
export declare const TOOL_NAME: "run_nyra";
export declare const TOOL_DESCRIPTION: string;
export declare const PARAMETERS: {
  type: "object";
  properties: { code: { type: "string"; description: string }; stdin: { type: "string"; description: string } };
  required: ["code"];
  additionalProperties: false;
};
export declare function anthropicToolSpec(name?: string, description?: string): {
  name: string;
  description: string;
  input_schema: typeof PARAMETERS;
};
export declare function openaiToolSpec(name?: string, description?: string): {
  type: "function";
  function: { name: string; description: string; parameters: typeof PARAMETERS };
};
export declare function runTool(input: { code: string; stdin?: string }, sandbox?: Sandbox): Promise<string>;

// the raw WebAssembly compiler (as site/play/nyra.js)
export interface NyraWasm {
  version(): string;
  check(src: string): { ok: boolean; errors: Diagnostic[]; warnings?: Diagnostic[] };
  js(src: string): { ok: true; js: string } | { ok: false; errors: Diagnostic[] };
  run(src: string, stdin?: string, limits?: string): Record<string, unknown>;
  explain(code: string): Record<string, unknown>;
  fmt(src: string): Record<string, unknown>;
  fix(src: string): { ok: boolean; source: string; fixed: Diagnostic[]; errors: Diagnostic[] };
}
export declare function compileNyra(source?: SandboxOptions["wasm"]): Promise<WebAssembly.Module>;
export declare function instantiateNyra(module: WebAssembly.Module): Promise<NyraWasm>;
