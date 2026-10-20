// What both backends share: the shape of a result, its errors and the text for a model.

export const FILE = "main.nyra";
export const CAPABILITIES = ["fs", "input", "os", "net"];
/** Runtime errors that mean a limit stopped the program. */
export const LIMITS = { E0355: "fuel", E0356: "memory", E0357: "output", E0358: "depth", E0359: "time" };

/** An error or warning with the keys every caller can rely on (the others kept). */
export function diag(d) {
  const out = { ...d };
  out.code = d.code ?? "";
  out.message = d.message ?? "";
  out.line = d.line ?? 0;
  out.col = d.col ?? 0;
  out.hint = d.hint || "";
  out.fix = d.fix ?? null;
  if (typeof out.file === "string") out.file = out.file.split(/[\\/]/).pop();
  return out;
}

export function render(errors) {
  const lines = [];
  for (const e of errors) {
    lines.push(`${e.runtime ? "runtime error" : "error"}[${e.code}] line ${e.line}:${e.col}: ${e.message}`);
    if (e.hint) lines.push(`  hint: ${e.hint}`);
  }
  return lines.join("\n");
}

export class NyraError extends Error {
  constructor(message, result) {
    super(message);
    this.name = "NyraError";
    this.result = result;
  }
}

/** What a run did (see index.d.ts). */
export class Result {
  constructor(fields) {
    this.ok = fields.ok;
    this.stdout = fields.stdout ?? "";
    this.stderr = fields.stderr ?? "";
    this.exitCode = fields.exitCode;
    this.errors = fields.errors ?? [];
    this.warnings = fields.warnings ?? [];
    this.steps = fields.steps ?? null;
    this.ms = fields.ms ?? 0;
    this.stage = fields.stage ?? null;
    this.limit = fields.limit ?? null;
    this.backend = fields.backend;
  }

  /** A short text for a model: the output, then each error with its line and hint. */
  feedback(maxChars = 8000) {
    const parts = [];
    if (this.stage === "compile") parts.push("The program did not compile.");
    else if (this.limit) parts.push(`The program was stopped by the ${this.limit} limit (exit ${this.exitCode}).`);
    else if (!this.ok) parts.push(`The program failed (exit ${this.exitCode}).`);
    let out = this.stdout;
    if (out.length > maxChars) out = out.slice(0, maxChars) + `\n... (${this.stdout.length - maxChars} more characters)`;
    if (out || this.ok) parts.push(out ? "stdout:\n" + out : "stdout: (empty)");
    if (this.errors.length) parts.push("errors:\n" + render(this.errors));
    const fixed = this.warnings.filter((w) => w.applied);
    if (fixed.length) {
      parts.push("repaired in memory (write it this way next time):\n" +
        fixed.map((w) => `  line ${w.line}: ${w.message} -> ${w.applied}`).join("\n"));
    }
    return parts.join("\n");
  }
}

/** Fills `stage`, `limit` and `stderr` from the errors. */
export function finish(fields) {
  const errors = fields.errors ?? [];
  const stage = fields.stage ?? (errors.length ? "runtime" : null);
  const limit = errors.map((e) => LIMITS[e.code]).find(Boolean) ?? null;
  const stderr = [render(errors), fields.stderr ?? ""].filter(Boolean).join("\n");
  return new Result({ ...fields, errors, stage, limit, stderr, ok: fields.exitCode === 0 && errors.length === 0 });
}

/** Parses the printed output as one JSON value: all of it, else its last line. */
export function parseJsonOutput(result) {
  if (!result.ok) throw new NyraError(result.feedback(), result);
  const text = result.stdout.trim();
  try {
    return JSON.parse(text);
  } catch (_) { /* the last line below */ }
  const lines = text.split("\n");
  try {
    return JSON.parse(lines[lines.length - 1]);
  } catch (_) {
    throw new NyraError(`the program printed no JSON value: ${JSON.stringify(text.slice(0, 200))}`, result);
  }
}

/** The options of a Sandbox, checked, with their defaults. */
export function options(o = {}) {
  let allow = o.allow ?? [];
  if (typeof allow === "string") allow = allow.split(/[\s,]+/).filter(Boolean);
  for (const cap of allow) {
    if (cap !== "all" && !CAPABILITIES.includes(cap)) {
      throw new TypeError(`unknown capability ${JSON.stringify(cap)}: use ${CAPABILITIES.join(", ")} or "all"`);
    }
  }
  const opts = {
    fuel: o.fuel ?? 100_000_000,
    timeoutMs: o.timeoutMs ?? 10_000,
    memoryMb: o.memoryMb ?? 256,
    maxOutput: o.maxOutput ?? 1 << 20,
    maxDepth: o.maxDepth ?? null,
    allow,
    cwd: o.cwd ?? null,
    strict: !!o.strict,
    native: !!o.native,
    backend: o.backend ?? "auto",
    binary: o.binary ?? null,
    wasm: o.wasm ?? null,
  };
  for (const k of ["fuel", "timeoutMs", "memoryMb", "maxOutput"]) {
    if (!(Number.isFinite(opts[k]) && opts[k] >= 1)) throw new RangeError(`${k} must be a number of at least 1`);
  }
  if (!["auto", "native", "wasm"].includes(opts.backend)) throw new TypeError(`backend must be "auto", "native" or "wasm"`);
  return opts;
}

export function caps(allow) {
  if (allow.includes("all")) return "all";
  return [...new Set(["input", ...allow])].sort().join(",");
}

/** Capabilities beyond `input`: the WebAssembly build cannot grant them. */
export const needsNative = (allow) => allow.some((c) => c !== "input");
