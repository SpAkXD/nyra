// ---- Nyra runtime (TypeScript) ------------------------------------------------------------
// The same behavior as nyra's JavaScript runtime, with types. Ints are JavaScript numbers (exact
// up to 2^53), a char is its code point (a number), arrays are JavaScript arrays with copy on write.

const ny_process: any = (globalThis as any).process;

/// The box of an `inout` parameter: the callee reads and writes `.v`, the caller copies it back.
type Ref<T> = { v: T };

const ny_file: string = @FILE@;

// Runtime errors: the entry point prints the message and exits with 101.
class NyPanic extends Error {}
function ny_panic(code: string, msg: string, hint: string, line: number, col: number): never {
    const json = ny_process !== undefined && ny_process.env.NYRA_JSON;
    const text = json
        ? JSON.stringify({ ok: false, errors: [{ code, message: msg, file: ny_file, line, col, hint, runtime: true }] })
        : `runtime error[${code}]: ${msg}\n  --> ${ny_file}:${line}:${col}\n  = hint: ${hint}\n  = explain: nyra explain ${code}`;
    throw new NyPanic(text);
}
function ny_oom(line: number, col: number): never {
    return ny_panic("E0249", "out of memory", "the program needs more memory than the system gave it", line, col);
}
// What the entry point reports: a Nyra runtime error, or the engine running out of memory
// (E0249, without a position). Anything else is a bug in nyra and is thrown on.
function ny_rescue(e: unknown): Error {
    if (e instanceof NyPanic) return e;
    if (e instanceof RangeError && /invalid (string|array|typed array) length|allocation failed/i.test(e.message)) {
        try { ny_oom(0, 0); } catch (p) { return p as Error; }
    }
    throw e;
}

// `print(x, end: "")`: text without a newline (a browser has no stdout: the console gets a line).
function ny_write(s: string): void {
    if (ny_process !== undefined) ny_process.stdout.write(s);
    else console.log(s);
}

// ---- ints: every int is a safe integer (|n| <= 2^53 - 1), where JavaScript numbers are exact.
// A result beyond 64 bits is E0255, as on every backend; one that only leaves the safe range is
// E0256: the other backends hold it, a JavaScript number would round it.
function ny_int_range(what: string, r: number, line: number, col: number): never {
    if (r >= 9223372036854775808 || r < -9223372036854775808) {
        return ny_panic("E0255", "int overflow: " + what + " does not fit in 64 bits", "an int holds -9223372036854775808 to 9223372036854775807: use smaller values, or keep a running value small with `%` (e.g. `h = (h * 31 + x) % 1000000007`)", line, col);
    }
    return ny_unsafe_int(what, line, col);
}
function ny_unsafe_int(what: string, line: number, col: number): never {
    return ny_panic("E0256", what + " is outside the range of ints JavaScript represents exactly (-9007199254740991 to 9007199254740991)", "JavaScript numbers hold ints exactly only up to 2^53 - 1: run this program on the native (C), Rust, Go or Python target, where an int has 64 bits, or keep the values smaller", line, col);
}
function ny_add(a: number, b: number, line: number, col: number): number {
    const r = a + b;
    return Number.isSafeInteger(r) ? r : ny_int_range(a + " + " + b, r, line, col);
}
function ny_sub(a: number, b: number, line: number, col: number): number {
    const r = a - b;
    return Number.isSafeInteger(r) ? r : ny_int_range(a + " - " + b, r, line, col);
}
function ny_mul(a: number, b: number, line: number, col: number): number {
    const r = a * b + 0;
    return Number.isSafeInteger(r) ? r : ny_int_range(a + " * " + b, r, line, col);
}
// (the negation of a safe integer is one too)
function ny_neg(a: number, line: number, col: number): number {
    return 0 - a;
}
// ---- ints: `+ 0` turns -0 into 0 (an int never prints -0) ----
function ny_div(a: number, b: number, line: number, col: number): number {
    if (b === 0) ny_panic("E0241", "division by zero", "check the divisor first", line, col);
    return Math.trunc(a / b) + 0;
}
function ny_mod(a: number, b: number, line: number, col: number): number {
    if (b === 0) ny_panic("E0241", "division by zero", "check the divisor first", line, col);
    return (a % b) + 0;
}
// `for i in a..b step k`: a step of 0 would never end.
function ny_check_step(k: number, line: number, col: number): void {
    if (k === 0) ny_panic("E0243", "range step must not be 0", "use a positive step to count up and a negative one to count down", line, col);
}
// `opt.unwrap()` of `none`.
function ny_check_some(has: boolean, line: number, col: number): void {
    if (!has) ny_panic("E0350", "unwrap() of none", "check `x != none` first, or give a default with `x ?? value`", line, col);
}
// `xs.min()` / `xs.max()` of an empty array (`n` elements seen; `max` says which method).
function ny_check_non_empty(n: number, max: number, line: number, col: number): void {
    if (n === 0) ny_panic("E0247", max ? "max() of an empty array" : "min() of an empty array", "an empty array has no smallest or largest element: check `xs.len() > 0` first, or start from a value of your own with `fold`", line, col);
}
// int(x) of a float: truncates toward zero; NaN or a value outside the int range is an error.
function ny_f2i(x: number, line: number, col: number): number {
    if (Number.isNaN(x) || x >= 9223372036854775807 || x < -9223372036854775808) {
        ny_panic("E0245", "cannot convert " + String(x) + " to int",
            "int(x) needs a float that is not NaN and fits in an int", line, col);
    }
    const n = Math.trunc(x) + 0;
    return Number.isSafeInteger(n) ? n : ny_unsafe_int("int(" + String(x) + ")", line, col);
}

// ---- strings: Nyra counts characters (code points), JavaScript counts UTF-16 units ----
// Fast path: a string without surrogates has exactly one unit per character.
const NY_SURR = /[\uD800-\uDFFF]/;
function ny_len(s: string): number { return NY_SURR.test(s) ? Array.from(s).length : s.length; }
function ny_oob(i: number, n: number, line: number, col: number): never {
    return ny_panic("E0240", `index ${i} is out of bounds for length ${n}`,
        "valid indexes are 0 to len - 1; compare with `.len()` first", line, col);
}
function ny_range(a: number, b: number, n: number, line: number, col: number): void {
    if (a < 0 || a > b || b > n) {
        ny_panic("E0240", `range ${a}..${b} is out of bounds for length ${n}`,
            "a range a..b needs 0 <= a <= b <= len", line, col);
    }
}
// s[i]: a char is its code point
function ny_str_at(s: string, i: number, line: number, col: number): number {
    if (!NY_SURR.test(s)) {
        if (i < 0 || i >= s.length) ny_oob(i, s.length, line, col);
        return s.charCodeAt(i);
    }
    const cps = Array.from(s);
    if (i < 0 || i >= cps.length) ny_oob(i, cps.length, line, col);
    return cps[i].codePointAt(0)!;
}
function ny_str_slice(s: string, a: number, b: number, line: number, col: number): string {
    if (!NY_SURR.test(s)) {
        ny_range(a, b, s.length, line, col);
        return s.slice(a, b);
    }
    const cps = Array.from(s);
    ny_range(a, b, cps.length, line, col);
    return cps.slice(a, b).join("");
}
function ny_str_index_of(s: string, t: string): number {
    const j = s.indexOf(t);
    if (j < 0) return -1;
    return NY_SURR.test(s) ? Array.from(s.slice(0, j)).length : j;
}
// code point order (UTF-16 `<` differs for characters above U+FFFF)
function ny_str_cmp(a: string, b: string): number {
    if (!NY_SURR.test(a) && !NY_SURR.test(b)) return a < b ? -1 : a > b ? 1 : 0;
    const x = Array.from(a), y = Array.from(b);
    for (let i = 0; i < x.length && i < y.length; i++) {
        const p = x[i].codePointAt(0)!, q = y[i].codePointAt(0)!;
        if (p !== q) return p < q ? -1 : 1;
    }
    return x.length < y.length ? -1 : x.length > y.length ? 1 : 0;
}
function ny_str_replace(s: string, old: string, nw: string, line: number, col: number): string {
    if (old === "") ny_panic("E0243", "replace() needs a non-empty pattern", "the text to replace can't be \"\"", line, col);
    return s.split(old).join(nw);
}
function ny_str_trim(s: string): string { return s.replace(/^[ \t\n\r]+|[ \t\n\r]+$/g, ""); }
function ny_str_upper(s: string): string { return s.replace(/[a-z]+/g, (m) => m.toUpperCase()); }
function ny_str_lower(s: string): string { return s.replace(/[A-Z]+/g, (m) => m.toLowerCase()); }
function ny_str_repeat(s: string, n: number, line: number, col: number): string {
    if (n < 0) ny_panic("E0243", `repeat count must be >= 0, got ${n}`, "repeat(n) needs n >= 0", line, col);
    // the engine's longest string (the other runtimes stop at the same length)
    if (s.length * n > 536870888) ny_oom(line, col);
    return s.repeat(n);
}
// `s.pad_left(n, c)` / `s.pad_right(n, c)`: `c` added until `s` has `n` characters.
function ny_pad(s: string, n: number, c: number, left: boolean): string {
    const missing = n - ny_len(s);
    if (missing <= 0) return s;
    if (missing > 536870888) ny_oom(0, 0);
    const fill = String.fromCodePoint(c).repeat(missing);
    return left ? fill + s : s + fill;
}
// char tests: ASCII only, like upper() and lower()
function ny_char_is_digit(c: number): boolean { return c >= 48 && c <= 57; }
function ny_char_is_upper(c: number): boolean { return c >= 65 && c <= 90; }
function ny_char_is_lower(c: number): boolean { return c >= 97 && c <= 122; }
function ny_char_is_letter(c: number): boolean { return ny_char_is_upper(c) || ny_char_is_lower(c); }
function ny_char_upper(c: number): number { return c >= 97 && c <= 122 ? c - 32 : c; }
function ny_char_lower(c: number): number { return c >= 65 && c <= 90 ? c + 32 : c; }
function ny_is_space(c: number): boolean { return c === 32 || c === 9 || c === 10 || c === 13; }
function ny_char_str(c: number): string { return String.fromCodePoint(c); }
function ny_char_from(n: number, line: number, col: number): number {
    if (n < 0 || n > 1114111 || (n >= 55296 && n <= 57343)) {
        ny_panic("E0246", `char(${n}): not a valid character code`,
            "character codes go from 0 to 1114111, except 55296 to 57343", line, col);
    }
    return n;
}
// The text of a string in an error message: control characters as escapes (`\n`, `\u0000`).
function ny_shown(s: string): string {
    return s.replace(/[\x00-\x1f]/g, (c) =>
        c === "\n" ? "\\n" : c === "\t" ? "\\t" : c === "\r" ? "\\r" : "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
}
function ny_str_to_int(s: string, line: number, col: number): number {
    if (/^-?[0-9]+$/.test(s)) {
        const v = BigInt(s);
        if (v >= -9223372036854775808n && v <= 9223372036854775807n) {
            const n = Number(v) + 0;
            return Number.isSafeInteger(n) ? n : ny_unsafe_int(`int("${s}")`, line, col);
        }
    }
    return ny_panic("E0244", `cannot parse "${ny_shown(s)}" as int`, "int(s) accepts only digits with an optional `-`, e.g. \"-42\"", line, col);
}
function ny_str_to_float(s: string, line: number, col: number): number {
    if (!/^-?[0-9]+(\.[0-9]+)?([eE][+-]?[0-9]+)?$/.test(s)) {
        ny_panic("E0244", `cannot parse "${ny_shown(s)}" as float`,
            "float(s) accepts digits with an optional `-`, `.` part and exponent, e.g. \"-1.5e3\"", line, col);
    }
    return Number(s);
}
function ny_chars(s: string): number[] {
    const r: number[] = [];
    for (const c of s) r.push(c.codePointAt(0)!);
    return r;
}
function ny_split(s: string, sep: string, line: number, col: number): string[] {
    if (sep === "") {
        ny_panic("E0243", "split() needs a non-empty separator", "for the characters of a string use `s.chars()`", line, col);
    }
    return s.split(sep);
}
function ny_join_chars(a: number[], sep: string): string {
    let s = "";
    for (let i = 0; i < a.length; i++) {
        if (i) s += sep;
        s += String.fromCodePoint(a[i]);
    }
    return s;
}

// ---- arrays and structs: copy on write ----
// A value with more than one owner is marked shared (`ny_s`). A write copies a shared value
// first, so two variables never see each other's changes. Numbers and strings are values already.
function ny_share<T>(v: T): T {
    if (v !== null && typeof v === "object") (v as any).ny_s = true;
    return v;
}
// Marks every element of a new array shared (it shares them with another array) and returns it.
function ny_share_all<T>(a: T[]): T[] {
    for (let i = 0; i < a.length; i++) ny_share(a[i]);
    return a;
}
// A copy of one level: the copy is not shared, the values it now shares are.
function ny_copy<T>(v: T): T {
    if (Array.isArray(v)) return ny_share_all(v.slice()) as T;
    if (v instanceof Map) {
        const m = new Map();
        for (const [k, x] of v) m.set(k, ny_share(x));
        return m as T;
    }
    return (v as any).ny_cp();
}
// `v` itself when it has one owner, else a copy: what a write needs.
function ny_unique<T>(v: T): T {
    return (v as any).ny_s ? ny_copy(v) : v;
}
// `obj[key]` made unique for a write (copied and stored back when it was shared).
function ny_unique_in(obj: any, key: string | number): any {
    let v = obj[key];
    if (v.ny_s) {
        v = ny_copy(v);
        obj[key] = v;
    }
    return v;
}
// Index checks (E0240): `ny_ck` returns the index, `ny_get` the element.
function ny_ck<T>(a: T[], i: number, line: number, col: number): number {
    if (i < 0 || i >= a.length) ny_oob(i, a.length, line, col);
    return i;
}
function ny_get<T>(a: T[], i: number, line: number, col: number): T {
    if (i < 0 || i >= a.length) ny_oob(i, a.length, line, col);
    return a[i];
}
function ny_pop<T>(a: T[], line: number, col: number): T {
    if (a.length === 0) ny_panic("E0242", "pop() on an empty array", "check `xs.len() > 0` first", line, col);
    return a.pop()!;
}
function ny_insert<T>(a: T[], i: number, v: T, line: number, col: number): void {
    if (i < 0 || i > a.length) {
        ny_panic("E0240", `index ${i} is out of bounds for length ${a.length}`, "insert(i, x) needs 0 <= i <= len", line, col);
    }
    a.splice(i, 0, v);
}
function ny_remove<T>(a: T[], i: number, line: number, col: number): T {
    ny_ck(a, i, line, col);
    return a.splice(i, 1)[0];
}
function ny_slice<T>(a: T[], from: number, to: number, line: number, col: number): T[] {
    ny_range(from, to, a.length, line, col);
    return ny_share_all(a.slice(from, to));
}
function ny_concat<T>(a: T[], b: T[]): T[] { return ny_share_all(a.concat(b)); }
function ny_repeat<T>(a: T[], n: number, line: number, col: number): T[] {
    if (n < 0) ny_panic("E0243", `repeat count must be >= 0, got ${n}`, "repeat(n) needs n >= 0", line, col);
    // the other runtimes stop at the same length
    if (a.length * n > 100000000) ny_oom(line, col);
    const r: T[] = [];
    for (let k = 0; k < n; k++) for (let i = 0; i < a.length; i++) r.push(a[i]);
    return ny_share_all(r);
}
function ny_swap<T>(a: T[], i: number, j: number, line: number, col: number): void {
    ny_ck(a, i, line, col);
    ny_ck(a, j, line, col);
    const t = a[i];
    a[i] = a[j];
    a[j] = t;
}
// `xs += ys` on a unique `a`; `xs += xs` doubles it.
function ny_append<T>(a: T[], b: T[]): void {
    const n = b.length;
    for (let i = 0; i < n; i++) a.push(ny_share(b[i]));
}
// Deep equality, element by element (no shortcut for the same array: NaN never equals itself).
function ny_eq(a: any, b: any): boolean {
    if (a === null || typeof a !== "object") return a === b;
    if (Array.isArray(a)) {
        if (a.length !== b.length) return false;
        for (let i = 0; i < a.length; i++) if (!ny_eq(a[i], b[i])) return false;
        return true;
    }
    if (a instanceof Map) {
        if (a.size !== b.size) return false;
        for (const [k, v] of a) if (!b.has(k) || !ny_eq(v, b.get(k))) return false;
        return true;
    }
    return a.ny_eq(b);
}
// ---- maps: JavaScript Maps (insertion order), copied on write like arrays ----
// `[k: v, ...]`: the keys and values alternate.
function ny_mnew<K, V>(kv: any[]): Map<K, V> {
    const m = new Map<K, V>();
    for (let i = 0; i < kv.length; i += 2) m.set(kv[i], kv[i + 1]);
    return m;
}
// `m[k]`: E0248 when the key is missing (`kt` is the key's type, for the message).
function ny_mget<K, V>(m: Map<K, V>, k: K, kt: string, line: number, col: number): V {
    if (!m.has(k)) ny_panic("E0248", `key ${ny_fmt(k, kt)} is not in the map`, "check with `m.has(k)` first, or read it with `m.get(k, default)`", line, col);
    return m.get(k) as V;
}
function ny_mgetor<K, V>(m: Map<K, V>, k: K, d: V): V { return m.has(k) ? (m.get(k) as V) : d; }
function ny_index_of<T>(a: T[], v: T): number {
    for (let i = 0; i < a.length; i++) if (ny_eq(a[i], v)) return i;
    return -1;
}
// Top-down merge sort, the same algorithm as the other runtimes: stable, and it takes from the
// left unless the right element is smaller, so even NaN ends up in the same place.
function ny_sort<T>(a: T[], lt: (x: T, y: T) => boolean): void {
    if (a.length < 2) return;
    const tmp: T[] = new Array(a.length);
    const sort = (lo: number, hi: number): void => {
        if (hi - lo < 2) return;
        const mid = lo + Math.floor((hi - lo) / 2);
        sort(lo, mid);
        sort(mid, hi);
        let i = lo, j = mid;
        for (let k = lo; k < hi; k++) tmp[k] = j < hi && (i >= mid || lt(a[j], a[i])) ? a[j++] : a[i++];
        for (let k = lo; k < hi; k++) a[k] = tmp[k];
    };
    sort(0, a.length);
}
// `xs.sort_by(x => key)`: the same merge sort on the positions, ordered by the keys
function ny_sort_by<T, K>(a: T[], ks: K[], lt: (x: K, y: K) => boolean): void {
    const idx = Array.from(ks, (_, i) => i);
    ny_sort(idx, (i: number, j: number) => lt(ks[i], ks[j]));
    const old = a.slice();
    for (let i = 0; i < idx.length; i++) a[i] = old[idx[i]];
}
function ny_lt_num(x: number, y: number): boolean { return x < y; }
// NaN sorts after every number (and NaNs keep their order)
function ny_lt_float(x: number, y: number): boolean { return x < y || (y !== y && x === x); }
function ny_lt_str(x: string, y: string): boolean { return ny_str_cmp(x, y) < 0; }

// ---- printing: like Nyra code; `t` is the type, because a char is a number here ----
// "i" int, "f" float, "b" bool, "c" char, "s" str, "[" + the element type for an array, "S" struct.
const NY_ESC: { [c: string]: string } = { "\\": "\\\\", "\n": "\\n", "\t": "\\t", "\r": "\\r" };
function ny_quoted(text: string, quote: string): string {
    let s = quote;
    for (const c of text) s += c === quote ? "\\" + c : NY_ESC[c] || c;
    return s + quote;
}
function ny_fmt(v: any, t: string): string {
    switch (t[0]) {
        case "c": return ny_quoted(String.fromCodePoint(v), "'");
        case "s": return ny_quoted(v, '"');
        case "[": {
            const e = t.slice(1);
            let s = "[";
            for (let i = 0; i < v.length; i++) {
                if (i) s += ", ";
                s += ny_fmt(v[i], e);
            }
            return s + "]";
        }
        case "S": return v.ny_fmt();
        case "{": {
            // a map: "{" + the key type (one letter) + the value type
            if (v.size === 0) return "[:]";
            const kt = t[1], vt = t.slice(2);
            let s = "[";
            for (const [k, x] of v) {
                if (s.length > 1) s += ", ";
                s += ny_fmt(k, kt) + ": " + ny_fmt(x, vt);
            }
            return s + "]";
        }
        default: return String(v);
    }
}
