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
        const styles = m ? [] : ny_unstyle(n);
        const node = m || n, want = prev ? prev.nextSibling : old.firstChild;
        if (node !== want) old.insertBefore(node, want);
        for (const [e, t] of styles) ny_style(e, t);
        if (m) ny_patch(m, n);
        else if (n.nodeType === 1) ny_shown(n);
        prev = node;
    }
}
// A new node of a live region is on screen already: its motion words play now (the motion script
// reveals an element once, when it scrolls into view, by the class `m-in`).
function ny_shown(n) {
    const all = n.querySelectorAll ? [n, ...n.querySelectorAll("[data-anim]")] : [n];
    for (const e of all) if (e.hasAttribute("data-anim")) ny_class(e, ["m-in"]);
}
// Adds classes to an element (by its attribute: the runtime's tests run without a browser).
function ny_class(e, add) {
    const c = (e.getAttribute("class") || "").split(/\s+/).filter(Boolean);
    const more = add.filter((x) => !c.includes(x));
    if (more.length) e.setAttribute("class", c.concat(more).join(" "));
}
// Gives an element the declarations of a `style` attribute (`text`, or none) through the CSSOM: a
// page's policy forbids style attributes set by a script (no 'unsafe-inline'), never the CSSOM. A
// stagger child keeps its place (`--m-i`, set by the motion script).
function ny_style(e, text) {
    const want = new Map();
    for (const d of (text || "").split(";")) {
        const i = d.indexOf(":");
        if (i > 0) want.set(d.slice(0, i).trim(), d.slice(i + 1).trim());
    }
    const s = e.style;
    if (!s) {
        // no page (the runtime's tests): the attribute stands for the declarations
        const order = /--m-i:\s*([^;]+)/.exec(e.getAttribute("style") || "");
        if (order && !want.has("--m-i")) want.set("--m-i", order[1].trim());
        const t = Array.from(want, ([p, v]) => p + ":" + v).join(";");
        if (t) e.setAttribute("style", t);
        else e.removeAttribute("style");
        return;
    }
    for (const p of Array.from(s)) if (!want.has(p) && p !== "--m-i") s.removeProperty(p);
    for (const [p, v] of want) if (s.getPropertyValue(p).trim() !== v) s.setProperty(p, v);
}
// A view's HTML as the runtime parses it: each `style` attribute (`img(ratio:)`'s `--ratio`,
// `grid(8)`'s `--cols`) is renamed `data-ny-style`. The page's policy checks a style attribute
// when the HTML is parsed, also in a template, and drops it; the build makes the static page's
// ones classes, and the runtime gives these to the CSSOM. Text and attribute values never hold
// `<`, `>` or `"` (the library escapes them), so a tag is `<` up to the next `>`.
const NY_STYLE = "data-ny-style";
function ny_html(text) {
    return text.replace(/<[A-Za-z][^<>]*>/g, (tag) => tag.replace(/(\s)style=(?=["'])/, "$1" + NY_STYLE + "="));
}
// The declarations a fresh element of a view carries (see `ny_html`).
function ny_decl(e) {
    return e.hasAttribute(NY_STYLE) ? e.getAttribute(NY_STYLE) : e.getAttribute("style");
}
// A new element of a live region, before it goes on the page: its declarations (and those of the
// elements inside it) are taken off and given back as [element, text] pairs, for `ny_style` to set
// through the CSSOM once the element is on the page (set in the template's document, they would
// come along as a style attribute, which the policy drops).
function ny_unstyle(n) {
    const out = [];
    if (n.nodeType !== 1 || !n.querySelectorAll) return out;
    for (const e of [n, ...n.querySelectorAll("[style],[" + NY_STYLE + "]")]) {
        if (!e.hasAttribute("style") && !e.hasAttribute(NY_STYLE)) continue;
        out.push([e, ny_decl(e)]);
        e.removeAttribute("style");
        e.removeAttribute(NY_STYLE);
    }
    return out;
}
function ny_patch(a, b) {
    if (a.nodeType !== 1) {
        if (a.nodeValue !== b.nodeValue) a.nodeValue = b.nodeValue;
        return;
    }
    // what the motion script added is state, not markup: an element it revealed (`m-in`) stays
    // revealed, a stagger child keeps its place (`--m-i`)
    const motion = (a.getAttribute("class") || "").split(/\s+/).filter((c) => c.startsWith("m-"));
    // what the user typed or ticked is a property: it follows the new HTML only where the HTML
    // changed it, and never in the field that has the focus
    const busy = a === a.ownerDocument.activeElement;
    const was = [a.getAttribute("value"), a.hasAttribute("checked")];
    // an opened or closed <details> (an accordion's question) stays as the visitor left it
    const keep = (name) => name === "style" || name === NY_STYLE || (name === "open" && a.nodeName === "DETAILS");
    for (const at of Array.from(a.attributes)) if (!keep(at.name) && !b.hasAttribute(at.name)) a.removeAttribute(at.name);
    for (const at of Array.from(b.attributes)) if (!keep(at.name) && a.getAttribute(at.name) !== at.value) a.setAttribute(at.name, at.value);
    ny_class(a, motion);
    ny_style(a, ny_decl(b));
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
            t.innerHTML = ny_html(view());
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
    globalThis.nyra_web = { web: ny_web, morph: ny_morph, msg: ny_msg, html: ny_html };
}
