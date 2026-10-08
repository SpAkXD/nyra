
// ---- json: `json.str(v)` and `json.parse(text)` by the value's type ----
// A type is "i" int, "f" float, "b" bool, "c" char, "s" str, ["a", T] an array of T, or the class
// of a struct (its `ny_jf` lists [JSON name, property, type] of every field, in order).
function ny_jenc_str(s: any): any {
    let o = '"';
    for (const ch of s) {
        const c = ch.codePointAt(0);
        if (ch === '"') o += '\\"';
        else if (ch === "\\") o += "\\\\";
        else if (c < 0x20) {
            o += c === 8 ? "\\b" : c === 12 ? "\\f" : c === 10 ? "\\n" : c === 13 ? "\\r" : c === 9 ? "\\t"
                : "\\u" + c.toString(16).padStart(4, "0");
        } else o += ch;
    }
    return o + '"';
}
function ny_jenc(v: any, d: any): any {
    if (typeof d === "function") {
        const fs = d.ny_jf;
        let o = "{";
        for (let i = 0; i < fs.length; i++) {
            if (i) o += ",";
            o += ny_jenc_str(fs[i][0]) + ":" + ny_jenc(v[fs[i][1]], fs[i][2]);
        }
        return o + "}";
    }
    if (typeof d !== "string") {
        let o = "[";
        for (let i = 0; i < v.length; i++) {
            if (i) o += ",";
            o += ny_jenc(v[i], d[1]);
        }
        return o + "]";
    }
    switch (d) {
        case "i": return String(v);
        case "f": return Number.isFinite(v) ? String(v) : "null";
        case "b": return v ? "true" : "false";
        case "c": return ny_jenc_str(String.fromCodePoint(v));
        default: return ny_jenc_str(v);
    }
}
// The parser: the text, the position, the nesting and the path to the value being read.
function ny_jsyntax(p: any, what: any): any {
    let line = 1;
    for (let k = 0; k < p.i && k < p.s.length; k++) if (p.s.charCodeAt(k) === 10) line++;
    ny_panic("E0345", `json.parse: invalid JSON at line ${line}: ${what}`, "check the JSON text: it must be one value, with keys and strings in double quotes", p.line, p.col);
}
function ny_jpath(p: any): any {
    let s = "$";
    for (const seg of p.path) s += typeof seg === "number" ? `[${seg}]` : "." + seg;
    return s;
}
function ny_jtype(p: any, what: any): any {
    ny_panic("E0345", `json.parse: expected ${what} at ${ny_jpath(p)}`, "the JSON text must have the shape of the type it is read into", p.line, p.col);
}
function ny_jws(p: any): any {
    const s = p.s;
    while (p.i < s.length) {
        const c = s.charCodeAt(p.i);
        if (c === 32 || c === 9 || c === 10 || c === 13) p.i++;
        else break;
    }
}
// Skips white space to the start of a value: a syntax error unless one can start here.
function ny_jstart(p: any): any {
    ny_jws(p);
    if (p.i >= p.s.length) ny_jsyntax(p, "unexpected end of the text");
    const c = p.s[p.i];
    if (!"{[\"tfn-0123456789".includes(c)) ny_jsyntax(p, "expected a value");
    return c;
}
// `[` or `{`: true if a first element follows, false for an empty one (already closed).
function ny_jopen(p: any, open: any, what: any): any {
    if (ny_jstart(p) !== open) ny_jtype(p, what);
    if (++p.depth > 500) ny_jsyntax(p, "nested too deeply");
    p.i++;
    ny_jws(p);
    const close = open === "[" ? "]" : "}";
    if (p.s[p.i] === close) {
        p.i++;
        p.depth--;
        return false;
    }
    return true;
}
// After an element: true at `,` (another one follows), false at the closing bracket.
function ny_jnext(p: any, close: any): any {
    ny_jws(p);
    const c = p.s[p.i];
    if (c === ",") {
        p.i++;
        return true;
    }
    if (c === close) {
        p.i++;
        p.depth--;
        return false;
    }
    ny_jsyntax(p, p.i >= p.s.length ? "unexpected end of the text" : `expected \`,\` or \`${close}\``);
}
function ny_jhex(p: any, at: any): any {
    const h = p.s.slice(at, at + 4);
    return /^[0-9a-fA-F]{4}$/.test(h) ? parseInt(h, 16) : -1;
}
// A string; the position is at its `"`.
function ny_jstring(p: any): any {
    const s = p.s;
    let out = "";
    p.i++;
    for (;;) {
        if (p.i >= s.length) ny_jsyntax(p, "unterminated string");
        const c = s.charCodeAt(p.i);
        if (c === 34) {
            p.i++;
            return out;
        }
        if (c < 0x20) ny_jsyntax(p, "control character in a string");
        if (c !== 92) {
            out += s[p.i++];
            continue;
        }
        p.i++;
        if (p.i >= s.length) ny_jsyntax(p, "unterminated string");
        const e = s[p.i];
        const simple = ({ '"': '"', "\\": "\\", "/": "/", b: "\b", f: "\f", n: "\n", r: "\r", t: "\t" } as any)[e];
        if (simple !== undefined) {
            out += simple;
            p.i++;
            continue;
        }
        if (e !== "u") ny_jsyntax(p, "invalid escape");
        let cp = ny_jhex(p, p.i + 1);
        if (cp < 0 || (cp >= 0xDC00 && cp <= 0xDFFF)) ny_jsyntax(p, "invalid escape");
        p.i += 5;
        if (cp >= 0xD800 && cp <= 0xDBFF) {
            const lo = s[p.i] === "\\" && s[p.i + 1] === "u" ? ny_jhex(p, p.i + 2) : -1;
            if (lo < 0xDC00 || lo > 0xDFFF) ny_jsyntax(p, "invalid escape");
            cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
            p.i += 6;
        }
        out += String.fromCodePoint(cp);
    }
}
// A number: its text, and whether it has no fraction and no exponent.
function ny_jnumber(p: any): any {
    const s = p.s, start = p.i;
    const digit = () => p.i < s.length && s.charCodeAt(p.i) >= 48 && s.charCodeAt(p.i) <= 57;
    let whole = true;
    if (s[p.i] === "-") p.i++;
    if (s[p.i] === "0") p.i++;
    else if (digit()) while (digit()) p.i++;
    else ny_jsyntax(p, "invalid number");
    if (s[p.i] === ".") {
        p.i++;
        if (!digit()) ny_jsyntax(p, "invalid number");
        while (digit()) p.i++;
        whole = false;
    }
    if (s[p.i] === "e" || s[p.i] === "E") {
        p.i++;
        if (s[p.i] === "+" || s[p.i] === "-") p.i++;
        if (!digit()) ny_jsyntax(p, "invalid number");
        while (digit()) p.i++;
        whole = false;
    }
    return [s.slice(start, p.i), whole];
}
function ny_jliteral(p: any): any {
    for (const w of ["true", "false", "null"]) {
        if (p.s.startsWith(w, p.i)) {
            p.i += w.length;
            return w;
        }
    }
    ny_jsyntax(p, "expected a value");
}
// Any value, only checked (an object's fields that the type does not have).
function ny_jskip(p: any): any {
    const c = ny_jstart(p);
    if (c === "{" || c === "[") {
        if (ny_jopen(p, c, "")) {
            do {
                if (c === "{") ny_jkey(p);
                ny_jskip(p);
            } while (ny_jnext(p, c === "{" ? "}" : "]"));
        }
    } else if (c === '"') ny_jstring(p);
    else if (c === "-" || (c >= "0" && c <= "9")) ny_jnumber(p);
    else ny_jliteral(p);
}
// An object's key and the `:` after it.
function ny_jkey(p: any): any {
    ny_jws(p);
    if (p.i >= p.s.length) ny_jsyntax(p, "unexpected end of the text");
    if (p.s[p.i] !== '"') ny_jsyntax(p, "expected a string key");
    const k = ny_jstring(p);
    ny_jws(p);
    if (p.s[p.i] !== ":") ny_jsyntax(p, p.i >= p.s.length ? "unexpected end of the text" : "expected `:`");
    p.i++;
    return k;
}
function ny_jdec(p: any, d: any): any {
    const c = ny_jstart(p);
    if (typeof d === "function") {
        const fs = d.ny_jf, vals = new Array(fs.length), seen = new Array(fs.length).fill(false);
        if (ny_jopen(p, "{", "an object")) {
            do {
                const k = ny_jkey(p);
                const f = fs.findIndex((x) => x[0] === k);
                if (f < 0) {
                    ny_jskip(p);
                    continue;
                }
                p.path.push(k);
                vals[f] = ny_jdec(p, fs[f][2]);
                p.path.pop();
                seen[f] = true;
            } while (ny_jnext(p, "}"));
        }
        const missing = seen.indexOf(false);
        if (missing >= 0) {
            ny_panic("E0345", `json.parse: missing field "${fs[missing][0]}" at ${ny_jpath(p)}`,
                "the JSON object must have every field of the struct", p.line, p.col);
        }
        return new d(...vals);
    }
    if (typeof d !== "string") {
        const out: any[] = [];
        if (ny_jopen(p, "[", "an array")) {
            do {
                p.path.push(out.length);
                out.push(ny_jdec(p, d[1]));
                p.path.pop();
            } while (ny_jnext(p, "]"));
        }
        return out;
    }
    switch (d) {
        case "i": {
            if (c !== "-" && (c < "0" || c > "9")) ny_jtype(p, "an int");
            const [t, whole] = ny_jnumber(p);
            if (whole) {
                const v = BigInt(t);
                if (v >= -9223372036854775808n && v <= 9223372036854775807n) return Number(v) + 0;
            }
            p.i -= t.length;
            ny_jtype(p, "an int");
        }
        case "f": {
            if (c !== "-" && (c < "0" || c > "9")) ny_jtype(p, "a number");
            return Number(ny_jnumber(p)[0]);
        }
        case "b": {
            if (c !== "t" && c !== "f") ny_jtype(p, "true or false");
            return ny_jliteral(p) === "true";
        }
        case "c": {
            if (c !== '"') ny_jtype(p, "a one-character string");
            const start = p.i, s = ny_jstring(p), cps = Array.from(s);
            if (cps.length !== 1) {
                p.i = start;
                ny_jtype(p, "a one-character string");
            }
            return cps[0].codePointAt(0);
        }
        default: {
            if (c !== '"') ny_jtype(p, "a string");
            return ny_jstring(p);
        }
    }
}
function ny_jparse(text: any, d: any, line: any, col: any): any {
    const p: any = { s: text, i: 0, depth: 0, path: [], line, col };
    const v = ny_jdec(p, d);
    ny_jws(p);
    if (p.i < text.length) ny_jsyntax(p, "text after the value");
    return v;
}
