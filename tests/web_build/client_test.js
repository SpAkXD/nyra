// The client code of the test site (tests/web_build.rs), run in Node.js without a browser:
// `node client_test.js site.<hash>.js`. Outside a page the runtime hands out `ny_web` (update and
// the live views) and its morph as `globalThis.nyra_web`; the morph is tried on a tiny fake DOM.
"use strict";
const assert = require("assert");
require(require("path").resolve(process.argv[2]));
const { web, morph, msg } = globalThis.nyra_web;

// update and the live view
const first = web.views.menu();
assert(first.includes("Sourdough") && first.includes("Croissant") && first.includes("0 clicks, showing all"), first);
web.update(JSON.stringify({ Show: ["sweet"] }));
const sweet = web.views.menu();
assert(sweet.includes("Croissant") && !sweet.includes("Sourdough") && sweet.includes("1 clicks, showing sweet"), sweet);
web.update('"Clear"');
assert(web.views.menu().includes("2 clicks, showing all"));
// text that is no Msg stops that update and leaves the state as it was
assert.throws(() => web.update("not json"));
assert(web.views.menu().includes("2 clicks, showing all"));

// the keyed morph on a fake DOM
const doc = { activeElement: null };
class Node {
    constructor(type, name, value) {
        Object.assign(this, { nodeType: type, nodeName: name, nodeValue: value ?? null, childNodes: [], parentNode: null, attrs: new Map(), ownerDocument: doc });
    }
    get firstChild() { return this.childNodes[0] || null; }
    get nextSibling() {
        const p = this.parentNode;
        return p ? p.childNodes[p.childNodes.indexOf(this) + 1] || null : null;
    }
    get attributes() { return [...this.attrs].map(([name, value]) => ({ name, value })); }
    get id() { return this.attrs.get("id") || ""; }
    getAttribute(n) { return this.attrs.has(n) ? this.attrs.get(n) : null; }
    setAttribute(n, v) { this.attrs.set(n, String(v)); }
    removeAttribute(n) { this.attrs.delete(n); }
    hasAttribute(n) { return this.attrs.has(n); }
    insertBefore(n, ref) {
        if (n.parentNode) n.parentNode.removeChild(n);
        const i = ref ? this.childNodes.indexOf(ref) : this.childNodes.length;
        this.childNodes.splice(i, 0, n);
        n.parentNode = this;
        return n;
    }
    removeChild(n) {
        this.childNodes.splice(this.childNodes.indexOf(n), 1);
        n.parentNode = null;
        return n;
    }
    // only attribute selectors: "[a],[b]"
    querySelectorAll(sel) {
        const names = [...sel.matchAll(/\[([\w-]+)\]/g)].map((m) => m[1]);
        const out = [];
        const walk = (n) => {
            for (const c of n.childNodes) {
                if (c.nodeType !== 1) continue;
                if (names.some((x) => c.hasAttribute(x))) out.push(c);
                walk(c);
            }
        };
        walk(this);
        return out;
    }
}
const el = (name, attrs, ...kids) => {
    const e = new Node(1, name);
    for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, v);
    for (const k of kids) e.insertBefore(typeof k === "string" ? new Node(3, "#text", k) : k, null);
    return e;
};
const a = el("ARTICLE", { "data-key": "A" }, "a");
const b = el("ARTICLE", { "data-key": "B", class: "old" }, "b");
const p = el("P", {}, "x");
const field = el("INPUT", { value: "typed" });
field.value = "typed by the user";
const region = el("DIV", { "data-live": "menu" }, a, b, p, field);
doc.activeElement = field;
const fresh = el("#fragment", {}, el("ARTICLE", { "data-key": "B", class: "new" }, "b!"), el("ARTICLE", { "data-key": "C" }, "c"), el("P", {}, "y"), el("INPUT", { value: "from the program" }));
morph(region, fresh);
const kids = region.childNodes;
assert.strictEqual(kids.length, 4);
assert.strictEqual(kids[0], b, "the keyed node is kept, not made again");
assert.strictEqual(b.getAttribute("class"), "new");
assert.strictEqual(b.childNodes[0].nodeValue, "b!");
assert.strictEqual(kids[1].getAttribute("data-key"), "C");
assert.strictEqual(kids[2], p);
assert.strictEqual(p.childNodes[0].nodeValue, "y");
assert.strictEqual(kids[3], field, "the field with the focus stays");
assert.strictEqual(field.value, "typed by the user", "what the user is typing stays");
assert(!kids.includes(a), "a node that is gone is removed");

// what the motion script added stays: a revealed grid (`m-in`) and a stagger child's place (`--m-i`);
// a new node that moves is shown at once (it is on screen: nothing would reveal it)
const card = el("ARTICLE", { class: "card", style: "--m-i: 2" }, "old");
const grid = el("DIV", { class: "grid g3 m-in", "data-anim": "stagger" }, card);
const box = el("DIV", { "data-live": "menu" }, grid);
morph(box, el("#fragment", {}, el("DIV", { class: "grid g3", "data-anim": "stagger" }, el("ARTICLE", { class: "card" }, "new")), el("H3", { "data-anim": "rise" }, "t")));
assert.strictEqual(grid.getAttribute("class"), "grid g3 m-in", "a revealed grid stays revealed");
assert.strictEqual(card.getAttribute("style"), "--m-i:2");
assert.strictEqual(card.childNodes[0].nodeValue, "new");
assert.strictEqual(box.childNodes[1].getAttribute("class"), "m-in", "a new element with motion is revealed");

// a view's style attributes are renamed before the page parses its HTML (the page's policy drops a
// parsed one); the morph gives them to the element through the CSSOM (here: the attribute) once it
// is on the page, to a new element and to one that is kept
const { html } = globalThis.nyra_web;
assert.strictEqual(
    html('<div class="grid" style="--cols:8;"><figure class="fig" style="--ratio:4/3"><img alt="a style=&quot;b&quot;"></figure></div><p>style</p>'),
    '<div class="grid" data-ny-style="--cols:8;"><figure class="fig" data-ny-style="--ratio:4/3"><img alt="a style=&quot;b&quot;"></figure></div><p>style</p>'
);
const kept = el("DIV", { class: "grid s-1a2b" });
const room = el("DIV", { "data-live": "rooms" }, kept);
morph(room, el("#fragment", {}, el("DIV", { class: "grid", "data-ny-style": "--cols:8;" }), el("FIGURE", { "data-ny-style": "--ratio:4/3" }, el("I", { "data-ny-style": "--tint:red" }))));
assert.strictEqual(room.childNodes[0], kept);
assert.strictEqual(kept.getAttribute("class"), "grid");
assert.strictEqual(kept.getAttribute("style"), "--cols:8");
assert(!kept.hasAttribute("data-ny-style"));
const fig = room.childNodes[1];
assert.strictEqual(fig.getAttribute("style"), "--ratio:4/3");
assert.strictEqual(fig.childNodes[0].getAttribute("style"), "--tint:red");
assert(!fig.hasAttribute("data-ny-style") && !fig.childNodes[0].hasAttribute("data-ny-style"));
// an accordion question the visitor opened stays open when its view renders again
const q = el("DETAILS", { class: "acc-item", open: "" });
const faq = el("DIV", { "data-live": "faq" }, q);
morph(faq, el("#fragment", {}, el("DETAILS", { class: "acc-item" })));
assert.strictEqual(faq.childNodes[0], q);
assert(q.hasAttribute("open"));

// a form whose `data-on` is a variant name sends that variant with its value
const input = el("INPUT", { "data-on": '"Typed"' });
input.value = "hi";
assert.strictEqual(msg(input), '{"Typed":["hi"]}');
assert.strictEqual(msg(el("BUTTON", { "data-on": '{"Show":["x"]}' })), '{"Show":["x"]}');
console.log("ok");
