(() => {
(() => {
const nav = document.querySelector('.nav');
if (!nav) return;
const bar = nav.querySelector('.nav-progress');
let queued = false;
const onScroll = () => {
queued = false;
const y = window.scrollY;
nav.classList.toggle('is-scrolled', y > 40);
if (bar) {
const max = document.documentElement.scrollHeight - window.innerHeight;
bar.style.setProperty('--progress', max > 0 ? Math.min(y / max, 1).toFixed(4) : '0');
}
};
window.addEventListener('scroll', () => {
if (!queued) {
queued = true;
requestAnimationFrame(onScroll);
}
}, { passive: true });
onScroll();
const more = nav.querySelector('.nav-more');
if (more) {
const toggle = more.querySelector('.nav-toggle');
more.addEventListener('toggle', () => {
toggle.setAttribute('aria-label', more.open ? toggle.dataset.close : toggle.dataset.open);
toggle.setAttribute('aria-expanded', String(more.open));
});
toggle.setAttribute('aria-expanded', 'false');
more.addEventListener('click', (e) => {
if (e.target.closest('.nav-menu a')) more.open = false;
});
document.addEventListener('keydown', (e) => {
if (e.key === 'Escape' && more.open) {
more.open = false;
toggle.focus();
}
});
document.addEventListener('click', (e) => {
if (more.open && !nav.contains(e.target)) more.open = false;
});
window.matchMedia('(min-width: 1024px)').addEventListener('change', (m) => {
if (m.matches) more.open = false;
});
}
const links = [...nav.querySelectorAll('.nav-links a, .nav-menu ul a')].filter((a) => /^#./.test(a.getAttribute('href')));
const sections = [...new Set(links.map((a) => a.getAttribute('href').slice(1)))]
.map((id) => document.getElementById(id))
.filter(Boolean);
if (!sections.length || !('IntersectionObserver' in window)) return;
const io = new IntersectionObserver((entries) => {
for (const e of entries) {
if (!e.isIntersecting) continue;
for (const a of links) {
if (a.getAttribute('href') === '#' + e.target.id) a.setAttribute('aria-current', 'true');
else a.removeAttribute('aria-current');
}
}
}, { rootMargin: '-35% 0px -60% 0px' });
sections.forEach((s) => io.observe(s));
})();
(() => {
const DAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
const now = (tz) => {
try {
const parts = new Intl.DateTimeFormat('en-GB', {
timeZone: tz || undefined, weekday: 'short', hour: '2-digit', minute: '2-digit', hourCycle: 'h23',
}).formatToParts(new Date());
const get = (t) => (parts.find((p) => p.type === t) || {}).value;
const d = DAYS.indexOf(get('weekday'));
return { day: d === 0 ? 7 : d, min: Number(get('hour')) * 60 + Number(get('minute')) };
} catch {
const d = new Date();
return { day: d.getDay() || 7, min: d.getHours() * 60 + d.getMinutes() };
}
};
const days = (spec) => {
const out = new Set();
for (const part of spec.split(',')) {
const [a, b] = part.split('-').map(Number);
if (Number.isNaN(a)) continue;
for (let d = a; d <= (Number.isNaN(b) || b === undefined ? a : b); d++) out.add(d === 0 ? 7 : d);
}
return out;
};
const mins = (t) => {
const [h, m] = t.split(':').map(Number);
return h * 60 + (m || 0);
};
const inRange = (r, m) => {
const [a, b] = r.split('-');
if (!a || !b) return false;
const from = mins(a);
const to = mins(b);
return to > from ? m >= from && m < to : m >= from || m < to;
};
const openOn = (row, t) => {
const [d, ...ranges] = row.trim().split(/\s+/);
return days(d).has(t.day) && ranges.some((r) => inRange(r, t.min));
};
const update = () => {
for (const b of document.querySelectorAll('.badge[data-hours]')) {
const t = now(b.dataset.tz);
const open = b.dataset.hours.split('|').some((row) => row && openOn(row, t));
b.dataset.state = open ? 'open' : 'closed';
const label = b.querySelector('.badge-label');
if (label) label.textContent = open ? b.dataset.open : b.dataset.closed;
}
for (const card of document.querySelectorAll('.hours')) {
const badge = card.querySelector('.badge');
const t = now(badge && badge.dataset.tz);
for (const tr of card.querySelectorAll('tr[data-days]')) tr.classList.toggle('is-today', days(tr.dataset.days).has(t.day));
}
};
update();
setInterval(update, 60000);
})();
(() => {
document.addEventListener('submit', (e) => {
const form = e.target.closest && e.target.closest('.form-body[data-mailto]');
if (!form) return;
e.preventDefault();
if (!form.reportValidity()) return;
const fields = [...form.elements].filter((el) => el.name && !['submit', 'button'].includes(el.type));
const message = fields.find((el) => el.tagName === 'TEXTAREA');
const label = (el) => {
const l = el.closest('label') || form.querySelector(`label[for="${el.id}"]`);
return (l ? l.textContent : el.name).replace('*', '').trim();
};
const lines = fields
.filter((el) => el !== message && (el.type === 'checkbox' ? el.checked : el.value.trim()))
.map((el) => (el.type === 'checkbox' ? label(el) : el.value.trim()));
let body = (message ? message.value.trim() + '\n\n—\n' : '') + lines.join('\n');
const query = (form.dataset.query || '').split('&').filter((kv) => {
if (!/^body=/i.test(kv)) return kv !== '';
try {
body = decodeURIComponent(kv.slice(5)) + '\n\n' + body;
} catch {
body = kv.slice(5) + '\n\n' + body;
}
return false;
});
query.push('body=' + encodeURIComponent(body));
window.location.href = 'mailto:' + form.dataset.mailto + '?' + query.join('&');
const card = form.closest('.form');
const done = card && card.querySelector('.form-done');
if (done) {
form.hidden = true;
done.hidden = false;
const edit = done.querySelector('.form-edit');
if (edit) edit.focus();
}
});
document.addEventListener('click', (e) => {
const edit = e.target.closest('.form-edit');
const card = edit && edit.closest('.form');
if (!card) return;
card.querySelector('.form-body').hidden = false;
card.querySelector('.form-done').hidden = true;
const first = card.querySelector('input, textarea, select');
if (first) first.focus();
});
})();
})();
(()=>{
if(!globalThis.document)return;
const D=document,R=D.documentElement,
RM=matchMedia('(prefers-reduced-motion: reduce)').matches||R.dataset.motion=='off',
$=(s,r=D)=>[...r.querySelectorAll(s)],
setup=[],on=[],subs=new Set();
let raf,last;
R.classList.add('m-on');
const frame=(t)=>{
for(const f of subs)f(Math.min(t-last,50),t)||subs.delete(f);
last=t;
raf=subs.size&&!D.hidden&&requestAnimationFrame(frame);
};
const wake=()=>raf||D.hidden||(last=performance.now(),raf=requestAnimationFrame(frame));
const loop=(f)=>{subs.add(f);wake();};
D.addEventListener('visibilitychange',wake);
const show=(el)=>{
if(el.classList.contains('m-in'))return;
el.classList.add('m-in');
for(const f of on)f(el);
const box=el.matches('[data-anim~=stagger],[data-anim~=draw]');
for(const c of $('[data-anim]',el))
c.parentElement.closest('[data-anim]')==el&&(box||c.hasAttribute('data-at'))&&show(c);
};
const io=new IntersectionObserver((es)=>{
for(const e of es)if(e.isIntersecting){io.unobserve(e.target);show(e.target);}
},{rootMargin:'0px 0px -8% 0px'});
const init=()=>{
for(const c of $('[data-anim~=stagger],[data-anim~=draw]'))
[...c.children].forEach((k,i)=>k.style.setProperty('--m-i',i));
for(const f of setup)f();
for(const el of $('[data-anim]')){
if(RM||el.matches('[data-anim~=none]'))show(el);
else if(!el.hasAttribute('data-at'))io.observe(el);
else if(!el.parentElement.closest('[data-anim]'))requestAnimationFrame(()=>show(el));
}
};
D.readyState=='loading'?D.addEventListener('DOMContentLoaded',init):init();
})();
(() => {
"use strict";
const ny_file = "site.nyra";
class NyPanic extends Error {}
function ny_panic(code, msg, hint, line, col) {
const json = typeof process !== "undefined" && process.env.NYRA_JSON;
const text = json
? JSON.stringify({ ok: false, errors: [{ code, message: msg, file: ny_file, line, col, hint, runtime: true }] })
: `runtime error[${code}]: ${msg}\n  --> ${ny_file}:${line}:${col}\n  = hint: ${hint}\n  = explain: nyra explain ${code}`;
throw new NyPanic(text);
}
function ny_oob(i, n, line, col) {
ny_panic("E0240", `index ${i} is out of bounds for length ${n}`,
"valid indexes are 0 to len - 1; compare with `.len()` first", line, col);
}
function ny_shown(s) {
return s.replace(/[\x00-\x1f]/g, (c) =>
c === "\n" ? "\\n" : c === "\t" ? "\\t" : c === "\r" ? "\\r" : "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
}
function ny_sh(v) {
if (v !== null && typeof v === "object") v.ny_s = true;
return v;
}
function ny_shall(a) {
for (let i = 0; i < a.length; i++) ny_sh(a[i]);
return a;
}
function ny_cp(a) {
if (Array.isArray(a)) return ny_shall(a.slice());
if (a instanceof Map) {
const m = new Map();
for (const [k, v] of a) m.set(k, ny_sh(v));
return m;
}
return a.ny_cp();
}
function ny_ck(a, i, line, col) {
if (i < 0 || i >= a.length) ny_oob(i, a.length, line, col);
return i;
}
function ny_get(a, i, line, col) {
if (i < 0 || i >= a.length) ny_oob(i, a.length, line, col);
return a[i];
}
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
const NY_KEYS = new Map();
function ny_kstr(v) {
if (typeof v === "string") return JSON.stringify(v);
if (v === null || typeof v !== "object") return String(v);
return "(" + Object.keys(v).filter((f) => !f.startsWith("ny_")).map((f) => ny_kstr(v[f])).join(",") + ")";
}
function ny_kcopy(v) {
if (v === null || typeof v !== "object") return v;
const c = v.ny_cp();
for (const f of Object.keys(c)) if (!f.startsWith("ny_")) c[f] = ny_kcopy(c[f]);
c.ny_s = true;
return c;
}
function ny_ik(k) {
if (k === null || typeof k !== "object") return k;
const s = ny_kstr(k);
let c = NY_KEYS.get(s);
if (c === undefined) {
c = ny_kcopy(k);
NY_KEYS.set(s, c);
}
return c;
}
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
function ny_jdef(d) {
if (typeof d === "function") return new d(...d.ny_jf.map((f) => ny_jdef(f[2])));
if (typeof d !== "string") return d[0] === "m" ? new Map() : [];
return d === "s" ? "" : d === "b" ? false : 0;
}
class nyS_Msg {
constructor(p0, p1, p2) { this.tag = p0; this.FilterRooms_0 = p1; this.ShowFaq_0 = p2; }
ny_cp() { return new nyS_Msg(this.tag, this.FilterRooms_0, this.ShowFaq_0); }
ny_eq(o) { return this.tag === o.tag && this.FilterRooms_0 === o.FilterRooms_0 && this.ShowFaq_0 === o.ShowFaq_0; }
ny_fmt() { const n = ["Msg.FilterRooms", "Msg.ShowFaq"][this.tag]; if (this.tag === 0) return n + "(" + ny_fmt(this.FilterRooms_0, "s") + ")"; if (this.tag === 1) return n + "(" + ny_fmt(this.ShowFaq_0, "i") + ")"; return n; }
}
class nyS_Room {
constructor(p0, p1, p2, p3, p4, p5, p6, p7) { this.name = p0; this.type = p1; this.size = p2; this.guests = p3; this.price = p4; this.desc = p5; this.image = p6; this.alt = p7; }
ny_cp() { return new nyS_Room(this.name, this.type, this.size, this.guests, this.price, this.desc, this.image, this.alt); }
ny_eq(o) { return this.name === o.name && this.type === o.type && this.size === o.size && this.guests === o.guests && this.price === o.price && this.desc === o.desc && this.image === o.image && this.alt === o.alt; }
ny_fmt() { return "Room(" + "name: " + ny_fmt(this.name, "s") + ", " + "type: " + ny_fmt(this.type, "s") + ", " + "size: " + ny_fmt(this.size, "s") + ", " + "guests: " + ny_fmt(this.guests, "i") + ", " + "price: " + ny_fmt(this.price, "s") + ", " + "desc: " + ny_fmt(this.desc, "s") + ", " + "image: " + ny_fmt(this.image, "s") + ", " + "alt: " + ny_fmt(this.alt, "s") + ")"; }
}
class nyS_Review {
constructor(p0, p1, p2, p3) { this.text = p0; this.author = p1; this.place = p2; this.stars = p3; }
ny_cp() { return new nyS_Review(this.text, this.author, this.place, this.stars); }
ny_eq(o) { return this.text === o.text && this.author === o.author && this.place === o.place && this.stars === o.stars; }
ny_fmt() { return "Review(" + "text: " + ny_fmt(this.text, "s") + ", " + "author: " + ny_fmt(this.author, "s") + ", " + "place: " + ny_fmt(this.place, "s") + ", " + "stars: " + ny_fmt(this.stars, "i") + ")"; }
}
class nyS_FaqItem {
constructor(p0, p1) { this.q = p0; this.a = p1; }
ny_cp() { return new nyS_FaqItem(this.q, this.a); }
ny_eq(o) { return this.q === o.q && this.a === o.a; }
ny_fmt() { return "FaqItem(" + "q: " + ny_fmt(this.q, "s") + ", " + "a: " + ny_fmt(this.a, "s") + ")"; }
}
class nyS_WebConfig {
constructor(p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14) { this.name = p0; this.url = p1; this.lang = p2; this.tz = p3; this.desc = p4; this.share = p5; this.style = p6; this.motion = p7; this.transition = p8; this.font = p9; this.display = p10; this.primary = p11; this.logo = p12; this.say = p13; this.palette = p14; }
ny_cp() { return new nyS_WebConfig(this.name, this.url, this.lang, this.tz, this.desc, this.share, this.style, this.motion, this.transition, this.font, this.display, this.primary, this.logo, ny_sh(this.say), ny_sh(this.palette)); }
ny_eq(o) { return this.name === o.name && this.url === o.url && this.lang === o.lang && this.tz === o.tz && this.desc === o.desc && this.share === o.share && this.style === o.style && this.motion === o.motion && this.transition === o.transition && this.font === o.font && this.display === o.display && this.primary === o.primary && this.logo === o.logo && ny_eq(this.say, o.say) && ny_eq(this.palette, o.palette); }
ny_fmt() { return "WebConfig(" + "name: " + ny_fmt(this.name, "s") + ", " + "url: " + ny_fmt(this.url, "s") + ", " + "lang: " + ny_fmt(this.lang, "s") + ", " + "tz: " + ny_fmt(this.tz, "s") + ", " + "desc: " + ny_fmt(this.desc, "s") + ", " + "share: " + ny_fmt(this.share, "s") + ", " + "style: " + ny_fmt(this.style, "s") + ", " + "motion: " + ny_fmt(this.motion, "s") + ", " + "transition: " + ny_fmt(this.transition, "s") + ", " + "font: " + ny_fmt(this.font, "s") + ", " + "display: " + ny_fmt(this.display, "s") + ", " + "primary: " + ny_fmt(this.primary, "s") + ", " + "logo: " + ny_fmt(this.logo, "s") + ", " + "say: " + ny_fmt(this.say, "{ss") + ", " + "palette: " + ny_fmt(this.palette, "{ss") + ")"; }
}
class nyS_Node {
constructor(p0, p1) { this.html = p0; this.kit = p1; }
ny_cp() { return new nyS_Node(this.html, ny_sh(this.kit)); }
ny_eq(o) { return this.html === o.html && ny_eq(this.kit, o.kit); }
ny_fmt() { return "Node(" + "html: " + ny_fmt(this.html, "s") + ", " + "kit: " + ny_fmt(this.kit, "[s") + ")"; }
}
class nyS_Opt_of {
constructor(p0, p1) { this.has = p0; this.val = p1; }
ny_cp() { return new nyS_Opt_of(this.has, this.val); }
ny_eq(o) { return this.has === o.has && this.val === o.val; }
ny_fmt() { return this.has ? "Some(" + ny_fmt(this.val, "f") + ")" : "none"; }
}
class nyS_Tup_t2ss {
constructor(p0, p1) { this._0 = p0; this._1 = p1; }
ny_cp() { return new nyS_Tup_t2ss(this._0, this._1); }
ny_eq(o) { return this._0 === o._0 && this._1 === o._1; }
ny_fmt() { return "(" + ny_fmt(this._0, "s") + ", " + ny_fmt(this._1, "s") + ")"; }
}
class nyS_Opt_ot2ss {
constructor(p0, p1) { this.has = p0; this.val = p1; }
ny_cp() { return new nyS_Opt_ot2ss(this.has, ny_sh(this.val)); }
ny_eq(o) { return this.has === o.has && ny_eq(this.val, o.val); }
ny_fmt() { return this.has ? "Some(" + ny_fmt(this.val, "S") + ")" : "none"; }
}
class nyS_Tup_t3sss {
constructor(p0, p1, p2) { this._0 = p0; this._1 = p1; this._2 = p2; }
ny_cp() { return new nyS_Tup_t3sss(this._0, this._1, this._2); }
ny_eq(o) { return this._0 === o._0 && this._1 === o._1 && this._2 === o._2; }
ny_fmt() { return "(" + ny_fmt(this._0, "s") + ", " + ny_fmt(this._1, "s") + ", " + ny_fmt(this._2, "s") + ")"; }
}
class nyS_Opt_oi {
constructor(p0, p1) { this.has = p0; this.val = p1; }
ny_cp() { return new nyS_Opt_oi(this.has, this.val); }
ny_eq(o) { return this.has === o.has && this.val === o.val; }
ny_fmt() { return this.has ? "Some(" + ny_fmt(this.val, "i") + ")" : "none"; }
}
nyS_Msg.ny_jf = [["tag", "tag", "i"], ["FilterRooms_0", "FilterRooms_0", "s"], ["ShowFaq_0", "ShowFaq_0", "i"]];
nyS_Msg.ny_jk = "e"; nyS_Msg.ny_jn = "Msg"; nyS_Msg.ny_jv = [["FilterRooms", [1]], ["ShowFaq", [2]]];
nyS_Room.ny_jf = [["name", "name", "s"], ["type", "type", "s"], ["size", "size", "s"], ["guests", "guests", "i"], ["price", "price", "s"], ["desc", "desc", "s"], ["image", "image", "s"], ["alt", "alt", "s"]];
nyS_Review.ny_jf = [["text", "text", "s"], ["author", "author", "s"], ["place", "place", "s"], ["stars", "stars", "i"]];
nyS_FaqItem.ny_jf = [["q", "q", "s"], ["a", "a", "s"]];
nyS_WebConfig.ny_jf = [["name", "name", "s"], ["url", "url", "s"], ["lang", "lang", "s"], ["tz", "tz", "s"], ["desc", "desc", "s"], ["share", "share", "s"], ["style", "style", "s"], ["motion", "motion", "s"], ["transition", "transition", "s"], ["font", "font", "s"], ["display", "display", "s"], ["primary", "primary", "s"], ["logo", "logo", "s"], ["say", "say", ["m", "s", "s"]], ["palette", "palette", ["m", "s", "s"]]];
nyS_Node.ny_jf = [["html", "html", "s"], ["kit", "kit", ["a", "s"]]];
nyS_Opt_of.ny_jf = [["has", "has", "b"], ["val", "val", "f"]];
nyS_Opt_of.ny_jk = "o";
nyS_Tup_t2ss.ny_jf = [["_0", "_0", "s"], ["_1", "_1", "s"]];
nyS_Tup_t2ss.ny_jk = "t";
nyS_Opt_ot2ss.ny_jf = [["has", "has", "b"], ["val", "val", nyS_Tup_t2ss]];
nyS_Opt_ot2ss.ny_jk = "o";
nyS_Tup_t3sss.ny_jf = [["_0", "_0", "s"], ["_1", "_1", "s"], ["_2", "_2", "s"]];
nyS_Tup_t3sss.ny_jk = "t";
nyS_Opt_oi.ny_jf = [["has", "has", "b"], ["val", "val", "i"]];
nyS_Opt_oi.ny_jk = "o";
function update(m, roomFilter, faqOpen) {
let nyU__ub7_m1, t, ny_t5, i, ny_t7;
nyU__ub7_m1 = m;
ny_sh(nyU__ub7_m1);
if (nyU__ub7_m1.tag === 0) {
t = nyU__ub7_m1.FilterRooms_0;
ny_t5 = t;
roomFilter.v = ny_t5;
} else {
i = nyU__ub7_m1.ShowFaq_0;
ny_t7 = ny_get(faqOpen.v, i, 13, 34);
{
if (faqOpen.v.ny_s) faqOpen.v = ny_cp(faqOpen.v);
faqOpen.v[ny_ck(faqOpen.v, i, 13, 20)] = !ny_t7;
}
}
}
const ny_st = {
"roomFilter": ny_from("All", "s"),
"faqOpen": ny_from([false,false,false,false], ["a", "b"]),
};
const ny_web = {
update(text) {
const ny_m = ny_from(JSON.parse(text), nyS_Msg);
const ny_b0 = {v: ny_st["roomFilter"]};
const ny_b1 = {v: ny_sh(ny_st["faqOpen"])};
update(ny_m, ny_b0, ny_b1);
ny_st["roomFilter"] = ny_b0.v;
ny_st["faqOpen"] = ny_b1.v;
},
views: {
},
};
function ny_std_web__emit() {}
function ny_std_web__uses() {}
function ny_std_web__css() {}
function ny_std_web__keyframes() {}
function ny_std_web__state() {}
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
if (!m) ny_restyle(n);
const node = m || n, want = prev ? prev.nextSibling : old.firstChild;
if (node !== want) old.insertBefore(node, want);
if (m) ny_patch(m, n);
else if (n.nodeType === 1) ny_shown(n);
prev = node;
}
}
function ny_shown(n) {
const all = n.querySelectorAll ? [n, ...n.querySelectorAll("[data-anim]")] : [n];
for (const e of all) if (e.hasAttribute("data-anim")) ny_class(e, ["m-in"]);
}
function ny_class(e, add) {
const c = (e.getAttribute("class") || "").split(/\s+/).filter(Boolean);
const more = add.filter((x) => !c.includes(x));
if (more.length) e.setAttribute("class", c.concat(more).join(" "));
}
function ny_style(e, text) {
const want = new Map();
for (const d of (text || "").split(";")) {
const i = d.indexOf(":");
if (i > 0) want.set(d.slice(0, i).trim(), d.slice(i + 1).trim());
}
const s = e.style;
if (!s) {
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
function ny_restyle(n) {
if (n.nodeType !== 1 || !n.querySelectorAll) return;
for (const e of [n, ...n.querySelectorAll("[style]")]) {
if (!e.hasAttribute("style")) continue;
const t = e.getAttribute("style");
e.removeAttribute("style");
ny_style(e, t);
}
}
function ny_patch(a, b) {
if (a.nodeType !== 1) {
if (a.nodeValue !== b.nodeValue) a.nodeValue = b.nodeValue;
return;
}
const motion = (a.getAttribute("class") || "").split(/\s+/).filter((c) => c.startsWith("m-"));
const busy = a === a.ownerDocument.activeElement;
const was = [a.getAttribute("value"), a.hasAttribute("checked")];
for (const at of Array.from(a.attributes)) if (at.name !== "style" && !b.hasAttribute(at.name)) a.removeAttribute(at.name);
for (const at of Array.from(b.attributes)) if (at.name !== "style" && a.getAttribute(at.name) !== at.value) a.setAttribute(at.name, at.value);
ny_class(a, motion);
ny_style(a, b.getAttribute("style"));
if (!busy && was[0] !== a.getAttribute("value")) a.value = a.getAttribute("value") || "";
if (was[1] !== a.hasAttribute("checked")) a.checked = a.hasAttribute("checked");
if (!(busy && a.nodeName === "TEXTAREA")) ny_morph(a, b);
}
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
globalThis.nyra_web = { web: ny_web, morph: ny_morph, msg: ny_msg };
}
})();
