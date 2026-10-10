// ---- strings: JS strings; Nyra counts characters (code points), JS counts UTF-16 units ----
// Fast path: a string without surrogates has exactly one unit per character.
const NY_SURR = /[\uD800-\uDFFF]/;
function ny_len(s) { return NY_SURR.test(s) ? Array.from(s).length : s.length; }
function ny_oob(i, n, line, col) {
    ny_panic("E0240", `index ${i} is out of bounds for length ${n}`,
        "valid indexes are 0 to len - 1; compare with `.len()` first", line, col);
}
function ny_range(a, b, n, line, col) {
    if (a < 0 || a > b || b > n) {
        ny_panic("E0240", `range ${a}..${b} is out of bounds for length ${n}`,
            "a range a..b needs 0 <= a <= b <= len", line, col);
    }
}
// s[i]: a char is its code point (a number)
function ny_str_at(s, i, line, col) {
    if (!NY_SURR.test(s)) {
        if (i < 0 || i >= s.length) ny_oob(i, s.length, line, col);
        return s.charCodeAt(i);
    }
    const cps = Array.from(s);
    if (i < 0 || i >= cps.length) ny_oob(i, cps.length, line, col);
    return cps[i].codePointAt(0);
}
function ny_str_slice(s, a, b, line, col) {
    if (!NY_SURR.test(s)) {
        ny_range(a, b, s.length, line, col);
        return s.slice(a, b);
    }
    const cps = Array.from(s);
    ny_range(a, b, cps.length, line, col);
    return cps.slice(a, b).join("");
}
function ny_str_index_of(s, t) {
    const j = s.indexOf(t);
    if (j < 0) return -1;
    return NY_SURR.test(s) ? Array.from(s.slice(0, j)).length : j;
}
// code point order (UTF-16 `<` differs for characters above U+FFFF)
function ny_str_cmp(a, b) {
    if (!NY_SURR.test(a) && !NY_SURR.test(b)) return a < b ? -1 : a > b ? 1 : 0;
    const x = Array.from(a), y = Array.from(b);
    for (let i = 0; i < x.length && i < y.length; i++) {
        const p = x[i].codePointAt(0), q = y[i].codePointAt(0);
        if (p !== q) return p < q ? -1 : 1;
    }
    return x.length < y.length ? -1 : x.length > y.length ? 1 : 0;
}
function ny_str_replace(s, old, nw, line, col) {
    if (old === "") ny_panic("E0243", "replace() needs a non-empty pattern", "the text to replace can't be \"\"", line, col);
    return s.split(old).join(nw);
}
function ny_str_trim(s) { return s.replace(/^[ \t\n\r]+|[ \t\n\r]+$/g, ""); }
function ny_str_upper(s) { return s.replace(/[a-z]+/g, (m) => m.toUpperCase()); }
function ny_str_lower(s) { return s.replace(/[A-Z]+/g, (m) => m.toLowerCase()); }
function ny_str_repeat(s, n, line, col) {
    if (n < 0) ny_panic("E0243", `repeat count must be >= 0, got ${n}`, "repeat(n) needs n >= 0", line, col);
    // the engine's longest string (the C runtime stops at the same length)
    if (s.length * n > 536870888) ny_oom(line, col);
    return s.repeat(n);
}
// char tests: ASCII only, like upper() and lower()
function ny_char_is_digit(c) { return c >= 48 && c <= 57; }
function ny_char_is_upper(c) { return c >= 65 && c <= 90; }
function ny_char_is_lower(c) { return c >= 97 && c <= 122; }
function ny_char_is_letter(c) { return ny_char_is_upper(c) || ny_char_is_lower(c); }
function ny_char_upper(c) { return c >= 97 && c <= 122 ? c - 32 : c; }
function ny_char_lower(c) { return c >= 65 && c <= 90 ? c + 32 : c; }
function ny_is_space(c) { return c === 32 || c === 9 || c === 10 || c === 13; }
function ny_char_str(c) { return String.fromCodePoint(c); }
// `s.pad_left(n, c)` / `s.pad_right(n, c)`: `c` added until `s` has `n` characters.
function ny_pad(s, n, c, left) {
    const missing = n - ny_len(s);
    if (missing <= 0) return s;
    if (missing > 536870888) ny_oom(0, 0);
    const fill = String.fromCodePoint(c).repeat(missing);
    return left ? fill + s : s + fill;
}
function ny_char_from(n, line, col) {
    if (n < 0 || n > 1114111 || (n >= 55296 && n <= 57343)) {
        ny_panic("E0246", `char(${n}): not a valid character code`,
            "character codes go from 0 to 1114111, except 55296 to 57343", line, col);
    }
    return n;
}
// The text of a string in an error message: control characters as escapes (`\n`, `\u0000`).
function ny_shown(s) {
    return s.replace(/[\x00-\x1f]/g, (c) =>
        c === "\n" ? "\\n" : c === "\t" ? "\\t" : c === "\r" ? "\\r" : "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
}
function ny_str_to_int(s, line, col) {
    if (/^-?[0-9]+$/.test(s)) {
        const v = BigInt(s);
        if (v >= -9223372036854775808n && v <= 9223372036854775807n) {
            const n = Number(v) + 0;
            return Number.isSafeInteger(n) ? n : ny_unsafe_int(`int("${s}")`, line, col);
        }
    }
    ny_panic("E0244", `cannot parse "${ny_shown(s)}" as int`, "int(s) accepts only digits with an optional `-`, e.g. \"-42\"", line, col);
}
function ny_str_to_float(s, line, col) {
    if (!/^-?[0-9]+(\.[0-9]+)?([eE][+-]?[0-9]+)?$/.test(s)) {
        ny_panic("E0244", `cannot parse "${ny_shown(s)}" as float`,
            "float(s) accepts digits with an optional `-`, `.` part and exponent, e.g. \"-1.5e3\"", line, col);
    }
    return Number(s);
}
