// ---- nyra web runtime: events to `update`, then the live regions re-rendered (src/web/runtime.js)
// `ny_web` (made by the build) has update(msgJson) and views[name]() -> HTML. An element with
// `data-on` sends its Msg; a form or field whose `data-on` is only a variant name ("Send") sends
// that variant with the form's fields or the field's value. After each update every element with
// `data-live` gets the new HTML of its view, patched in place (a keyed morph: `data-key` or `id`),
// so focus, selection and scroll stay. A runtime error stops that update and is logged.
function ny_std_web__emit() {}
function ny_std_web__uses() {}
function ny_std_web__css() {}
function ny_std_web__keyframes() {}
function ny_std_web__state() {}
// A value of the type `d` (see json.js) from what JSON.parse made of text the build wrote.
function ny_from(v, d) {
    if (typeof d === "function") {
        const fs = d.ny_jf, k = d.ny_jk;
        if (k === "o") return v === null ? new d(false, ny_jdef(fs[1][2])) : new d(true, ny_from(v, fs[1][2]));
        if (k === "t") return new d(...fs.map((f, i) => ny_from(v[i], f[2])));
        if (k !== "e") return new d(...fs.map((f) => ny_from(v[f[0]], f[2])));
        const name = typeof v === "string" ? v : Object.keys(v)[0];
        const x = d.ny_jv.findIndex((e) => e[0] === name);
        const a = fs.map((f) => ny_jdef(f[2]));
        a[0] = x;
        d.ny_jv[x][1].forEach((i, j) => { a[i] = ny_from(v[name][j], fs[i][2]); });
        return new d(...a);
    }
    if (typeof d !== "string") {
        if (d[0] === "a") return v.map((x) => ny_from(x, d[1]));
        const m = new Map();
        if (d[1] === "s") for (const k of Object.keys(v)) m.set(k, ny_from(v[k], d[2]));
        else for (const [k, x] of v) m.set(ny_ik(ny_from(k, d[1])), ny_from(x, d[2]));
        return m;
    }
    return d === "c" ? v.codePointAt(0) : v;
}
function ny_key(n) {
    return n.nodeType === 1 ? n.getAttribute("data-key") || n.id || null : null;
}
function ny_same(a, b) {
    return a.nodeType === b.nodeType && a.nodeName === b.nodeName;
}
// Makes the children of `old` those of `fresh` (a fragment), reusing the nodes that match: by key,
// else the next old node of the same kind. Nodes already in place are not moved (a move loses focus).
function ny_morph(old, fresh) {
    const keyed = new Map(), loose = [];
    for (const c of old.childNodes) {
        if (ny_key(c) && !keyed.has(ny_key(c))) keyed.set(ny_key(c), c);
        else loose.push(c);
    }
    let at = 0;
    const pairs = Array.from(fresh.childNodes, (n) => {
        const k = ny_key(n);
        let m = k ? keyed.get(k) : null;
        if (k) keyed.delete(k);
        while (!k && !m && at < loose.length) {
            const c = loose[at++];
            if (ny_same(c, n) && !ny_key(c)) m = c;
        }
        return [n, m && ny_same(m, n) ? m : null];
    });
    const used = new Set(pairs.map((p) => p[1]));
    for (const c of Array.from(old.childNodes)) if (!used.has(c)) old.removeChild(c);
    let prev = null;
    for (const [n, m] of pairs) {
        const node = m || n, want = prev ? prev.nextSibling : old.firstChild;
        if (node !== want) old.insertBefore(node, want);
        if (m) ny_patch(m, n);
        prev = node;
    }
}
function ny_patch(a, b) {
    if (a.nodeType !== 1) {
        if (a.nodeValue !== b.nodeValue) a.nodeValue = b.nodeValue;
        return;
    }
    // what the user typed or ticked is a property: it follows the new HTML only where the HTML
    // changed it, and never in the field that has the focus
    const busy = a === a.ownerDocument.activeElement;
    const was = [a.getAttribute("value"), a.hasAttribute("checked")];
    for (const at of Array.from(a.attributes)) if (!b.hasAttribute(at.name)) a.removeAttribute(at.name);
    for (const at of Array.from(b.attributes)) if (a.getAttribute(at.name) !== at.value) a.setAttribute(at.name, at.value);
    if (!busy && was[0] !== a.getAttribute("value")) a.value = a.getAttribute("value") || "";
    if (was[1] !== a.hasAttribute("checked")) a.checked = a.hasAttribute("checked");
    if (!(busy && a.nodeName === "TEXTAREA")) ny_morph(a, b);
}
// The Msg of an event as JSON.
function ny_msg(el) {
    const t = el.getAttribute("data-on");
    if (t[0] !== '"' || !/^(FORM|INPUT|TEXTAREA|SELECT)$/.test(el.nodeName)) return t;
    const v = el.nodeName === "FORM" ? Object.fromEntries(new FormData(el))
        : el.type === "checkbox" ? el.checked : el.value;
    return JSON.stringify({ [JSON.parse(t)]: [v] });
}
function ny_send(text) {
    try {
        ny_web.update(text);
    } catch (e) {
        console.error(e instanceof NyPanic ? e.message : e);
        return;
    }
    for (const el of document.querySelectorAll("[data-live]")) {
        const view = ny_web.views[el.getAttribute("data-live")];
        if (!view) continue;
        try {
            const t = document.createElement("template");
            t.innerHTML = view();
            ny_morph(el, t.content);
        } catch (e) {
            console.error(e instanceof NyPanic ? e.message : e);
        }
    }
}
if (typeof document !== "undefined") {
    document.addEventListener("click", (e) => {
        const el = e.target.closest && e.target.closest("[data-on]");
        if (!el || /^(FORM|INPUT|TEXTAREA|SELECT)$/.test(el.nodeName)) return;
        e.preventDefault();
        ny_send(ny_msg(el));
    });
    document.addEventListener("submit", (e) => {
        if (!e.target.hasAttribute("data-on")) return;
        e.preventDefault();
        ny_send(ny_msg(e.target));
    });
    document.addEventListener("input", (e) => {
        if (e.target.hasAttribute && e.target.hasAttribute("data-on")) ny_send(ny_msg(e.target));
    });
} else {
    // no page (a test in Node.js): hand the pieces out
    globalThis.nyra_web = { web: ny_web, morph: ny_morph, msg: ny_msg };
}
