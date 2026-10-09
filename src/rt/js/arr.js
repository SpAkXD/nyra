
// ---- arrays: JavaScript arrays with copy on write ----
// A value with more than one owner is marked shared (`ny_s`, like `rc > 1` in C). A write
// copies a shared value first, so two variables never see each other's changes. Only arrays
// (and structs) are marked; numbers and strings are values already.
function ny_sh(v) {
    if (v !== null && typeof v === "object") v.ny_s = true;
    return v;
}
// Marks every element of a new array shared (it shares them with another array) and returns it.
function ny_shall(a) {
    for (let i = 0; i < a.length; i++) ny_sh(a[i]);
    return a;
}
// A copy of one level: the copy is not shared, the elements it now shares are.
function ny_cp(a) {
    if (Array.isArray(a)) return ny_shall(a.slice());
    if (a instanceof Map) {
        const m = new Map();
        for (const [k, v] of a) m.set(k, ny_sh(v));
        return m;
    }
    return a.ny_cp();
}
// `obj[key]`, made unique for a write (copied and stored back when it was shared).
function ny_u(obj, key) {
    let v = obj[key];
    if (v.ny_s) {
        v = ny_cp(v);
        obj[key] = v;
    }
    return v;
}
// Index checks (E0240): `ny_ck` returns the index, `ny_get` the element.
function ny_ck(a, i, line, col) {
    if (i < 0 || i >= a.length) ny_oob(i, a.length, line, col);
    return i;
}
function ny_get(a, i, line, col) {
    if (i < 0 || i >= a.length) ny_oob(i, a.length, line, col);
    return a[i];
}
function ny_pop(a, line, col) {
    if (a.length === 0) ny_panic("E0242", "pop() on an empty array", "check `xs.len() > 0` first", line, col);
    return a.pop();
}
function ny_insert(a, i, v, line, col) {
    if (i < 0 || i > a.length) {
        ny_panic("E0240", `index ${i} is out of bounds for length ${a.length}`, "insert(i, x) needs 0 <= i <= len", line, col);
    }
    a.splice(i, 0, v);
}
function ny_remove(a, i, line, col) {
    ny_ck(a, i, line, col);
    return a.splice(i, 1)[0];
}
function ny_aslice(a, from, to, line, col) {
    ny_range(from, to, a.length, line, col);
    return ny_shall(a.slice(from, to));
}
function ny_aconcat(a, b) { return ny_shall(a.concat(b)); }
function ny_arep(a, n, line, col) {
    if (n < 0) ny_panic("E0243", `repeat count must be >= 0, got ${n}`, "repeat(n) needs n >= 0", line, col);
    // the C runtime stops at the same length
    if (a.length * n > 100000000) ny_oom(line, col);
    const r = [];
    for (let k = 0; k < n; k++) for (let i = 0; i < a.length; i++) r.push(a[i]);
    return ny_shall(r);
}
function ny_swap(a, i, j, line, col) {
    ny_ck(a, i, line, col);
    ny_ck(a, j, line, col);
    const t = a[i];
    a[i] = a[j];
    a[j] = t;
}
// `xs += ys` on a unique `a`; `xs += xs` doubles it.
function ny_append(a, b) {
    const n = b.length;
    for (let i = 0; i < n; i++) a.push(ny_sh(b[i]));
}
// Deep equality, element by element (no shortcut for the same array, like C: NaN never equals itself).
function ny_eq(a, b) {
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
function ny_mnew(kv) {
    const m = new Map();
    for (let i = 0; i < kv.length; i += 2) m.set(kv[i], kv[i + 1]);
    return m;
}
// `m[k]`: E0248 when the key is missing (`kt` is the key's type, for the message).
function ny_mget(m, k, kt, line, col) {
    if (!m.has(k)) ny_panic("E0248", `key ${ny_fmt(k, kt)} is not in the map`, "check with `m.has(k)` first, or read it with `m.get(k, default)`", line, col);
    return m.get(k);
}
function ny_mgetor(m, k, d) { return m.has(k) ? m.get(k) : d; }
// `m[k]`, made unique for a change in place (copied and stored back when it was shared).
function ny_mu(m, k, kt, line, col) {
    let v = ny_mget(m, k, kt, line, col);
    if (v !== null && typeof v === "object" && v.ny_s) {
        v = ny_cp(v);
        m.set(k, v);
    }
    return v;
}
function ny_aindex(a, v) {
    for (let i = 0; i < a.length; i++) if (ny_eq(a[i], v)) return i;
    return -1;
}
// Top-down merge sort, the same algorithm as the C runtime: stable, and it takes from the left
// unless the right element is smaller, so even NaN ends up in the same place.
function ny_sort(a, lt) {
    if (a.length < 2) return;
    const tmp = new Array(a.length);
    const sort = (lo, hi) => {
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
function ny_sort_by(a, ks, lt) {
    const idx = Array.from(ks, (_, i) => i);
    ny_sort(idx, (i, j) => lt(ks[i], ks[j]));
    const old = a.slice();
    for (let i = 0; i < idx.length; i++) a[i] = old[idx[i]];
}
function ny_lt_num(x, y) { return x < y; }
// NaN sorts after every number (and NaNs keep their order), like the C runtime
function ny_lt_float(x, y) { return x < y || (y !== y && x === x); }
function ny_lt_str(x, y) { return ny_str_cmp(x, y) < 0; }
function ny_join_char(a, sep) {
    let s = "";
    for (let i = 0; i < a.length; i++) {
        if (i) s += sep;
        s += String.fromCodePoint(a[i]);
    }
    return s;
}
function ny_chars(s) {
    const r = [];
    for (const c of s) r.push(c.codePointAt(0));
    return r;
}
function ny_split(s, sep, line, col) {
    if (sep === "") {
        ny_panic("E0243", "split() needs a non-empty separator", "for the characters of a string use `s.chars()`", line, col);
    }
    return s.split(sep);
}
// Printing by type, because a char is a number here: `t` is "i" int, "f" float, "b" bool,
// "c" char, "s" str, "[" + the element type for an array, "S" for a struct.
const NY_ESC = { "\\": "\\\\", "\n": "\\n", "\t": "\\t", "\r": "\\r" };
function ny_escaped(text, quote) {
    let s = quote;
    for (const c of text) s += c === quote ? "\\" + c : NY_ESC[c] || c;
    return s + quote;
}
function ny_fmt(v, t) {
    switch (t[0]) {
        case "c": return ny_escaped(String.fromCodePoint(v), "'");
        case "s": return ny_escaped(v, '"');
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
