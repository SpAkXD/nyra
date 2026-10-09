
// ---- standard library: `use input`, `use os`, `use fs`, `use time`, `use random`, `use math`, `use text`
// Node's own modules (a browser has no files, input or arguments: those calls see none).
const ny_fs: any = ny_process !== undefined && ny_process.getBuiltinModule ? ny_process.getBuiltinModule("fs") : null;
const ny_Buffer: any = (globalThis as any).Buffer;
const ny_utf8 = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true });
// `os.exit(n)` unwinds to the entry point, which sets the exit code (output is never cut off).
class NyExit extends Error {
    code: number;
    constructor(code: number) { super("exit"); this.code = code; }
}
// Bytes as text, or null when they are not valid UTF-8.
function ny_text(bytes: Uint8Array): string | null {
    try { return ny_utf8.decode(bytes); } catch (e) { return null; }
}

// ---- fs ----
function ny_fs_fail(what: string, path: string, reason: string, line: number, col: number): never {
    return ny_panic("E0340", `${what} "${ny_shown(path)}" (${reason})`,
        "check the path: it is relative to the folder the program runs in (`fs.exists(path)` tests first)", line, col);
}
function ny_errno_reason(e: any): string {
    switch (e && e.code) {
        case "ENOENT": case "ENOTDIR": return "not found";
        case "EACCES": case "EPERM": return "permission denied";
        case "EEXIST": return "already exists";
        case "ENOTEMPTY": return "not empty";
        default: return "io error";
    }
}
// 0: nothing there (also for "" and a path with a NUL), 1: a file, 2: a directory.
function ny_kind(path: string): number {
    if (path === "" || path.includes("\0") || ny_fs === null) return 0;
    try { return ny_fs.statSync(path).isDirectory() ? 2 : 1; } catch (e) { return 0; }
}
function ny_std_fs_read(path: string, line: number, col: number): string {
    const what = "fs.read: cannot read", k = ny_kind(path);
    if (k === 0) ny_fs_fail(what, path, "not found", line, col);
    if (k === 2) ny_fs_fail(what, path, "is a directory", line, col);
    let bytes: Uint8Array = new Uint8Array(0);
    try { bytes = ny_fs.readFileSync(path); } catch (e) { ny_fs_fail(what, path, ny_errno_reason(e), line, col); }
    const s = ny_text(bytes);
    if (s === null) return ny_fs_fail(what, path, "not valid UTF-8", line, col);
    return s;
}
function ny_fs_put(path: string, text: string, append: boolean, what: string, line: number, col: number): void {
    if (path === "" || path.includes("\0") || ny_fs === null) ny_fs_fail(what, path, "not found", line, col);
    if (ny_kind(path) === 2) ny_fs_fail(what, path, "is a directory", line, col);
    try {
        if (append) ny_fs.appendFileSync(path, text); else ny_fs.writeFileSync(path, text);
    } catch (e) { ny_fs_fail(what, path, ny_errno_reason(e), line, col); }
}
function ny_std_fs_write(path: string, text: string, line: number, col: number): void { ny_fs_put(path, text, false, "fs.write: cannot write", line, col); }
function ny_std_fs_append(path: string, text: string, line: number, col: number): void { ny_fs_put(path, text, true, "fs.append: cannot append to", line, col); }
function ny_std_fs_exists(path: string, line: number, col: number): boolean { return ny_kind(path) !== 0; }
function ny_std_fs_list(dir: string, line: number, col: number): string[] {
    const what = "fs.list: cannot list", k = ny_kind(dir);
    if (k === 0) ny_fs_fail(what, dir, "not found", line, col);
    if (k === 1) ny_fs_fail(what, dir, "not a directory", line, col);
    let names: Uint8Array[] = [];
    try { names = ny_fs.readdirSync(dir, { encoding: "buffer" }); } catch (e) { ny_fs_fail(what, dir, ny_errno_reason(e), line, col); }
    const out: string[] = [];
    for (const b of names) {
        const s = ny_text(b);
        if (s === null) return ny_fs_fail(what, dir, "not valid UTF-8", line, col);
        out.push(s);
    }
    return out.sort(ny_str_cmp);
}
function ny_std_fs_remove(path: string, line: number, col: number): void {
    const what = "fs.remove: cannot remove", k = ny_kind(path);
    if (k === 0) ny_fs_fail(what, path, "not found", line, col);
    if (k === 2 && ny_std_fs_list(path, line, col).length > 0) ny_fs_fail(what, path, "not empty", line, col);
    try {
        if (k === 2) ny_fs.rmdirSync(path); else ny_fs.unlinkSync(path);
    } catch (e) { ny_fs_fail(what, path, ny_errno_reason(e), line, col); }
}
function ny_std_fs_mkdir(path: string, line: number, col: number): void {
    const what = "fs.mkdir: cannot create";
    if (path === "" || path.includes("\0") || ny_fs === null) ny_fs_fail(what, path, "not found", line, col);
    if (ny_kind(path) !== 0) ny_fs_fail(what, path, "already exists", line, col);
    try { ny_fs.mkdirSync(path); } catch (e) { ny_fs_fail(what, path, ny_errno_reason(e), line, col); }
}

// ---- input: standard input as bytes (the same on every system) ----
let ny_in: any = null, ny_in_pos = 0, ny_in_len = 0, ny_in_end = false;
// Reads more input; false at the end. Reads what is there (a line typed at a terminal).
function ny_in_fill(): boolean {
    if (ny_in_end || ny_fs === null) return false;
    if (ny_in === null) ny_in = ny_Buffer.allocUnsafe(65536);
    if (ny_in_pos > 0) {
        ny_in.copy(ny_in, 0, ny_in_pos, ny_in_len);
        ny_in_len -= ny_in_pos;
        ny_in_pos = 0;
    }
    if (ny_in_len === ny_in.length) {
        const bigger = ny_Buffer.allocUnsafe(ny_in.length * 2);
        ny_in.copy(bigger, 0, 0, ny_in_len);
        ny_in = bigger;
    }
    for (;;) {
        let n = 0;
        try {
            n = ny_fs.readSync(0, ny_in, ny_in_len, ny_in.length - ny_in_len, null);
        } catch (e: any) {
            if (e.code === "EAGAIN") {
                Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 5);
                continue;
            }
            n = 0;   // EOF (Windows) and other errors end the input
        }
        if (n <= 0) {
            ny_in_end = true;
            return false;
        }
        ny_in_len += n;
        return true;
    }
}
function ny_in_text(start: number, end: number, what: string, line: number, col: number): string {
    const s = ny_text(ny_in === null ? new Uint8Array(0) : ny_in.subarray(start, end));
    if (s === null) return ny_panic("E0341", `${what}: the input is not valid UTF-8`, "standard input must be UTF-8 text", line, col);
    return s;
}
function ny_std_input_line(line: number, col: number): string {
    let scan = ny_in_pos;
    for (;;) {
        const at = ny_in === null ? -1 : ny_in.subarray(scan, ny_in_len).indexOf(10);
        if (at >= 0) {
            const start = ny_in_pos, nl = scan + at;
            let end = nl;
            ny_in_pos = nl + 1;
            if (end > start && ny_in[end - 1] === 13) end--;
            return ny_in_text(start, end, "input.line", line, col);
        }
        scan = ny_in_len - ny_in_pos;
        if (!ny_in_fill()) break;
    }
    const start = ny_in_pos;
    ny_in_pos = ny_in_len;
    return ny_in_text(start, ny_in_len, "input.line", line, col);
}
function ny_std_input_eof(line: number, col: number): boolean {
    while (ny_in_pos === ny_in_len) if (!ny_in_fill()) return true;
    return false;
}
function ny_std_input_all(line: number, col: number): string {
    while (ny_in_fill()) {}
    const start = ny_in_pos;
    ny_in_pos = ny_in_len;
    return ny_in_text(start, ny_in_len, "input.all", line, col);
}
// The lines of a text: each without its `\n` (and a `\r` before it); no last empty line.
function ny_lines(s: string): string[] {
    const out: string[] = [];
    let i = 0;
    while (i < s.length) {
        const nl = s.indexOf("\n", i);
        let end = nl < 0 ? s.length : nl;
        const next = nl < 0 ? s.length : nl + 1;
        if (nl >= 0 && end > i && s.charCodeAt(end - 1) === 13) end--;
        out.push(s.slice(i, end));
        i = next;
    }
    return out;
}
function ny_std_input_lines(line: number, col: number): string[] {
    while (ny_in_fill()) {}
    const start = ny_in_pos;
    ny_in_pos = ny_in_len;
    return ny_lines(ny_in_text(start, ny_in_len, "input.lines", line, col));
}

// ---- os ----
function ny_std_os_args(line: number, col: number): string[] {
    return ny_process !== undefined ? ny_process.argv.slice(2) : [];
}
function ny_getenv(name: string): string | undefined {
    if (ny_process === undefined || name === "" || name.includes("=") || name.includes("\0")) return undefined;
    return ny_process.env[name];
}
function ny_std_os_env(name: string, line: number, col: number): string {
    const v = ny_getenv(name);
    return v === undefined ? "" : v;
}
function ny_std_os_has_env(name: string, line: number, col: number): boolean { return ny_getenv(name) !== undefined; }
function ny_std_os_exit(code: number, line: number, col: number): void { throw new NyExit(code & 255); }

// ---- time ----
function ny_std_time_now_ms(line: number, col: number): number { return Date.now(); }
function ny_std_time_mono_ms(line: number, col: number): number { return performance.now(); }
function ny_std_time_sleep_ms(ms: number, line: number, col: number): void {
    if (ms > 0) Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

// ---- random: the operating system's generator, or after `random.seed(n)` xoshiro128** ----
let ny_seeded = false, ny_pool: Uint32Array = new Uint32Array(64), ny_pool_left = 0;
const ny_rs = new Uint32Array(4);
function ny_rotl(x: number, k: number): number { return ((x << k) | (x >>> (32 - k))) >>> 0; }
function ny_mix32(z: number): number {
    z = Math.imul(z ^ (z >>> 16), 0x85EBCA6B);
    z = Math.imul(z ^ (z >>> 13), 0xC2B2AE35);
    return (z ^ (z >>> 16)) >>> 0;
}
function ny_bits32(): number {
    if (ny_seeded) {
        const s = ny_rs;
        const r = Math.imul(ny_rotl(Math.imul(s[1], 5) >>> 0, 7), 9) >>> 0, t = (s[1] << 9) >>> 0;
        s[2] ^= s[0]; s[3] ^= s[1]; s[1] ^= s[2]; s[0] ^= s[3]; s[2] ^= t; s[3] = ny_rotl(s[3], 11);
        return r;
    }
    if (ny_pool_left === 0) {
        (globalThis as any).crypto.getRandomValues(ny_pool);
        ny_pool_left = 64;
    }
    return ny_pool[--ny_pool_left];
}
function ny_std_random_seed(n: number, line: number, col: number): void {
    const half = [n >>> 0, Math.floor(n / 4294967296) >>> 0];
    for (let i = 0; i < 4; i++) ny_rs[i] = ny_mix32((half[i & 1] + Math.imul(i + 1, 0x9E3779B9)) >>> 0);
    if ((ny_rs[0] | ny_rs[1] | ny_rs[2] | ny_rs[3]) === 0) ny_rs[0] = 1;
    ny_seeded = true;
}
// 53 random bits: the first draw gives the high 27, the second the low 26.
function ny_bits53(): number {
    const a = ny_bits32() >>> 5;
    const b = ny_bits32() >>> 6;
    return a * 67108864 + b;
}
function ny_std_random_random(line: number, col: number): number { return ny_bits53() / 9007199254740992; }
function ny_std_random_range(lo: number, hi: number, line: number, col: number): number {
    if (hi <= lo || hi - lo > 9007199254740992) {
        ny_panic("E0342", `random.range(${lo}, ${hi}): need lo < hi and hi - lo <= 2^53`,
            "the upper bound is excluded: `random.range(1, 7)` rolls a die", line, col);
    }
    const n = hi - lo, limit = 9007199254740992 - (9007199254740992 % n);
    let r: number;
    do r = ny_bits53(); while (r >= limit);
    return lo + r % n;
}

// ---- math: operations that are exact on every host ----
function ny_std_math_sqrt(x: number, line: number, col: number): number { return Math.sqrt(x); }
function ny_std_math_floor(x: number, line: number, col: number): number { return Math.floor(x); }
function ny_std_math_ceil(x: number, line: number, col: number): number { return Math.ceil(x); }
function ny_std_math_trunc(x: number, line: number, col: number): number { return Math.trunc(x); }
// half away from zero (x - trunc(x) is exact)
function ny_std_math_round(x: number, line: number, col: number): number {
    let t = Math.trunc(x);
    if (Math.abs(x - t) >= 0.5) t += x < 0 ? -1 : 1;
    return t;
}

// ---- text ----
function ny_std_text_is_int(s: string, line: number, col: number): boolean {
    if (!/^-?[0-9]+$/.test(s)) return false;
    const v = BigInt(s);
    return v >= -9223372036854775808n && v <= 9223372036854775807n;
}
function ny_std_text_is_float(s: string, line: number, col: number): boolean { return /^-?[0-9]+(\.[0-9]+)?([eE][+-]?[0-9]+)?$/.test(s); }
// `text.fixed(x, d)`: the exact value of x rounded to d decimals, ties away from zero.
function ny_std_text_fixed(x: number, d: number, line: number, col: number): string {
    if (d < 0 || d > 100) ny_panic("E0342", `text.fixed: digits must be 0 to 100, got ${d}`, "`text.fixed(x, 2)` shows two decimals", line, col);
    if (!Number.isFinite(x)) return String(x);
    const v = new DataView(new ArrayBuffer(8));
    v.setFloat64(0, x);
    const hi = v.getUint32(0), lo = v.getUint32(4);
    let ex = (hi >>> 20) & 0x7FF;
    let m = (BigInt(hi & 0xFFFFF) << 32n) | BigInt(lo);
    if (ex === 0) ex = 1; else m |= 1n << 52n;
    const s = ex - 1075 + d;   // x * 10^d = m * 5^d * 2^s
    const a = m * 5n ** BigInt(d);
    const q = s >= 0 ? a << BigInt(s) : (a >> BigInt(-s)) + ((a >> BigInt(-s - 1)) & 1n);
    let digits = q.toString();
    if (d > 0) {
        digits = digits.padStart(d + 1, "0");
        digits = digits.slice(0, digits.length - d) + "." + digits.slice(digits.length - d);
    }
    return (hi >>> 31 && q !== 0n ? "-" : "") + digits;
}
