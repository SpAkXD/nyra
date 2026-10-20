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
(() => {
const ICON = {
x: '<path d="M18 6 6 18"/><path d="m6 6 12 12"/>',
prev: '<path d="m15 18-6-6 6-6"/>',
next: '<path d="m9 18 6-6-6-6"/>',
};
const svg = (k) => `<svg class="i" viewBox="0 0 24 24" aria-hidden="true">${ICON[k]}</svg>`;
for (const tabs of document.querySelectorAll('.gallery .tabs')) tabs.hidden = false;
const filter = (tab) => {
const g = tab.closest('.gallery');
const f = tab.dataset.filter;
for (const t of g.querySelectorAll('.tabs [data-filter]')) {
t.setAttribute('aria-selected', String(t === tab));
t.tabIndex = t === tab ? 0 : -1;
}
for (const it of g.querySelectorAll('.gallery-item')) it.hidden = Boolean(f) && it.dataset.tag !== f;
};
for (const t of document.querySelectorAll('.gallery .tabs [data-filter]')) t.tabIndex = t.getAttribute('aria-selected') === 'true' ? 0 : -1;
let box = null;
let items = [];
let index = 0;
let opener = null;
const show = (dir) => {
const it = items[index];
const img = it.querySelector('img');
const big = document.createElement('img');
big.src = it.getAttribute('href');
big.alt = img ? img.alt : '';
big.style.setProperty('--dir', String(dir));
box.querySelector('.lb-stage').replaceChildren(big);
box.querySelector('.lb-text').textContent = big.alt;
box.querySelector('.lb-count').textContent = `${index + 1} / ${items.length}`;
box.setAttribute('aria-label', big.alt);
};
const step = (d) => {
index = (index + d + items.length) % items.length;
show(d);
};
const build = (g) => {
box = document.createElement('dialog');
box.className = 'lightbox';
box.innerHTML = '<div class="lb-stage"></div><p class="lb-cap"><span class="lb-text"></span><span class="lb-count"></span></p>'
+ `<button type="button" class="lb-btn lb-close">${svg('x')}</button>`
+ `<button type="button" class="lb-btn lb-prev">${svg('prev')}</button>`
+ `<button type="button" class="lb-btn lb-next">${svg('next')}</button>`;
box.querySelector('.lb-close').setAttribute('aria-label', g.dataset.dismiss || 'Close');
box.querySelector('.lb-prev').setAttribute('aria-label', g.dataset.prev || 'Previous');
box.querySelector('.lb-next').setAttribute('aria-label', g.dataset.next || 'Next');
box.addEventListener('click', (e) => {
if (e.target.closest('.lb-close')) box.close();
else if (e.target.closest('.lb-prev')) step(-1);
else if (e.target.closest('.lb-next')) step(1);
else if (!e.target.closest('img, .lb-cap')) box.close();
});
box.addEventListener('keydown', (e) => {
if (e.key === 'ArrowRight') step(1);
if (e.key === 'ArrowLeft') step(-1);
});
let x0 = null;
box.addEventListener('touchstart', (e) => { x0 = e.touches[0].clientX; }, { passive: true });
box.addEventListener('touchend', (e) => {
if (x0 === null) return;
const dx = e.changedTouches[0].clientX - x0;
x0 = null;
if (Math.abs(dx) > 50) step(dx < 0 ? 1 : -1);
});
box.addEventListener('close', () => {
document.documentElement.style.overflow = '';
if (opener) opener.focus();
});
document.body.append(box);
};
const open = (item) => {
const g = item.closest('.gallery');
items = [...g.querySelectorAll('.gallery-item:not([hidden])')];
index = items.indexOf(item);
opener = item;
if (!box) build(g);
show(0);
if (box.showModal) box.showModal();
else box.setAttribute('open', '');
document.documentElement.style.overflow = 'hidden';
box.querySelector('.lb-close').focus();
};
document.addEventListener('click', (e) => {
const tab = e.target.closest('.gallery .tabs [data-filter]');
if (tab) {
filter(tab);
return;
}
const item = e.target.closest('.gallery-item');
if (item && !e.metaKey && !e.ctrlKey && !e.shiftKey && e.button === 0) {
e.preventDefault();
open(item);
}
});
document.addEventListener('keydown', (e) => {
const tab = e.target.closest && e.target.closest('.gallery .tabs [data-filter]');
if (!tab || (e.key !== 'ArrowRight' && e.key !== 'ArrowLeft')) return;
const all = [...tab.parentElement.querySelectorAll('[data-filter]')];
const next = all[(all.indexOf(tab) + (e.key === 'ArrowRight' ? 1 : -1) + all.length) % all.length];
next.focus();
filter(next);
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
function ny_int_range(what, r, line, col) {
if (r >= 9223372036854775808 || r < -9223372036854775808) {
ny_panic("E0255", "int overflow: " + what + " does not fit in 64 bits", "an int holds -9223372036854775808 to 9223372036854775807: use smaller values, or keep a running value small with `%` (e.g. `h = (h * 31 + x) % 1000000007`)", line, col);
}
ny_unsafe_int(what, line, col);
}
function ny_unsafe_int(what, line, col) {
ny_panic("E0256", what + " is outside the range of ints JavaScript represents exactly (-9007199254740991 to 9007199254740991)", "JavaScript numbers hold ints exactly only up to 2^53 - 1: run this program on the native (C), Rust, Go or Python target, where an int has 64 bits, or keep the values smaller", line, col);
}
function ny_add(a, b, line, col) {
const r = a + b;
return Number.isSafeInteger(r) ? r : ny_int_range(a + " + " + b, r, line, col);
}
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
function ny_str_replace(s, old, nw, line, col) {
if (old === "") ny_panic("E0243", "replace() needs a non-empty pattern", "the text to replace can't be \"\"", line, col);
return s.split(old).join(nw);
}
function ny_str_trim(s) { return s.replace(/^[ \t\n\r]+|[ \t\n\r]+$/g, ""); }
function ny_str_lower(s) { return s.replace(/[A-Z]+/g, (m) => m.toLowerCase()); }
function ny_char_is_digit(c) { return c >= 48 && c <= 57; }
function ny_char_is_upper(c) { return c >= 65 && c <= 90; }
function ny_char_is_lower(c) { return c >= 97 && c <= 122; }
function ny_char_is_letter(c) { return ny_char_is_upper(c) || ny_char_is_lower(c); }
function ny_char_str(c) { return String.fromCodePoint(c); }
function ny_shown(s) {
return s.replace(/[\x00-\x1f]/g, (c) =>
c === "\n" ? "\\n" : c === "\t" ? "\\t" : c === "\r" ? "\\r" : "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
}
function ny_str_to_float(s, line, col) {
if (!/^-?[0-9]+(\.[0-9]+)?([eE][+-]?[0-9]+)?$/.test(s)) {
ny_panic("E0244", `cannot parse "${ny_shown(s)}" as float`,
"float(s) accepts digits with an optional `-`, `.` part and exponent, e.g. \"-1.5e3\"", line, col);
}
return Number(s);
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
function ny_aindex(a, v) {
for (let i = 0; i < a.length; i++) if (ny_eq(a[i], v)) return i;
return -1;
}
function ny_chars(s) {
const r = [];
for (const c of s) r.push(c.codePointAt(0));
return r;
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
function ny_std_text_is_float(s, line, col) { return /^-?[0-9]+(\.[0-9]+)?([eE][+-]?[0-9]+)?$/.test(s); }
function ny_jenc_str(s) {
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
function ny_jenc(v, d) {
if (typeof d === "function") {
const fs = d.ny_jf, k = d.ny_jk;
if (k === "t") return "[" + fs.map((f) => ny_jenc(v[f[1]], f[2])).join(",") + "]";
if (k === "o") return v[fs[0][1]] ? ny_jenc(v[fs[1][1]], fs[1][2]) : "null";
if (k === "e") {
const [name, idx] = d.ny_jv[v[fs[0][1]]];
if (idx.length === 0) return ny_jenc_str(name);
return "{" + ny_jenc_str(name) + ":[" + idx.map((i) => ny_jenc(v[fs[i][1]], fs[i][2])).join(",") + "]}";
}
let o = "{";
for (let i = 0; i < fs.length; i++) {
if (i) o += ",";
o += ny_jenc_str(fs[i][0]) + ":" + ny_jenc(v[fs[i][1]], fs[i][2]);
}
return o + "}";
}
if (typeof d !== "string" && d[0] === "m") {
let o = "", n = 0;
for (const [k, x] of v) {
if (n++) o += ",";
o += d[1] === "s" ? ny_jenc_str(k) + ":" + ny_jenc(x, d[2]) : "[" + ny_jenc(k, d[1]) + "," + ny_jenc(x, d[2]) + "]";
}
return d[1] === "s" ? "{" + o + "}" : "[" + o + "]";
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
function ny_jdef(d) {
if (typeof d === "function") return new d(...d.ny_jf.map((f) => ny_jdef(f[2])));
if (typeof d !== "string") return d[0] === "m" ? new Map() : [];
return d === "s" ? "" : d === "b" ? false : 0;
}
class nyS_Msg {
constructor(p0, p1) { this.tag = p0; this.Show_0 = p1; }
ny_cp() { return new nyS_Msg(this.tag, this.Show_0); }
ny_eq(o) { return this.tag === o.tag && this.Show_0 === o.Show_0; }
ny_fmt() { const n = ["Msg.Show"][this.tag]; if (this.tag === 0) return n + "(" + ny_fmt(this.Show_0, "s") + ")"; return n; }
}
class nyS_Node {
constructor(p0, p1) { this.html = p0; this.kit = p1; }
ny_cp() { return new nyS_Node(this.html, ny_sh(this.kit)); }
ny_eq(o) { return this.html === o.html && ny_eq(this.kit, o.kit); }
ny_fmt() { return "Node(" + "html: " + ny_fmt(this.html, "s") + ", " + "kit: " + ny_fmt(this.kit, "[s") + ")"; }
}
class nyS_WebConfig {
constructor(p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14) { this.name = p0; this.url = p1; this.lang = p2; this.tz = p3; this.desc = p4; this.share = p5; this.style = p6; this.motion = p7; this.transition = p8; this.font = p9; this.display = p10; this.primary = p11; this.logo = p12; this.say = p13; this.palette = p14; }
ny_cp() { return new nyS_WebConfig(this.name, this.url, this.lang, this.tz, this.desc, this.share, this.style, this.motion, this.transition, this.font, this.display, this.primary, this.logo, ny_sh(this.say), ny_sh(this.palette)); }
ny_eq(o) { return this.name === o.name && this.url === o.url && this.lang === o.lang && this.tz === o.tz && this.desc === o.desc && this.share === o.share && this.style === o.style && this.motion === o.motion && this.transition === o.transition && this.font === o.font && this.display === o.display && this.primary === o.primary && this.logo === o.logo && ny_eq(this.say, o.say) && ny_eq(this.palette, o.palette); }
ny_fmt() { return "WebConfig(" + "name: " + ny_fmt(this.name, "s") + ", " + "url: " + ny_fmt(this.url, "s") + ", " + "lang: " + ny_fmt(this.lang, "s") + ", " + "tz: " + ny_fmt(this.tz, "s") + ", " + "desc: " + ny_fmt(this.desc, "s") + ", " + "share: " + ny_fmt(this.share, "s") + ", " + "style: " + ny_fmt(this.style, "s") + ", " + "motion: " + ny_fmt(this.motion, "s") + ", " + "transition: " + ny_fmt(this.transition, "s") + ", " + "font: " + ny_fmt(this.font, "s") + ", " + "display: " + ny_fmt(this.display, "s") + ", " + "primary: " + ny_fmt(this.primary, "s") + ", " + "logo: " + ny_fmt(this.logo, "s") + ", " + "say: " + ny_fmt(this.say, "{ss") + ", " + "palette: " + ny_fmt(this.palette, "{ss") + ")"; }
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
class nyS_Tup_t7sssssss {
constructor(p0, p1, p2, p3, p4, p5, p6) { this._0 = p0; this._1 = p1; this._2 = p2; this._3 = p3; this._4 = p4; this._5 = p5; this._6 = p6; }
ny_cp() { return new nyS_Tup_t7sssssss(this._0, this._1, this._2, this._3, this._4, this._5, this._6); }
ny_eq(o) { return this._0 === o._0 && this._1 === o._1 && this._2 === o._2 && this._3 === o._3 && this._4 === o._4 && this._5 === o._5 && this._6 === o._6; }
ny_fmt() { return "(" + ny_fmt(this._0, "s") + ", " + ny_fmt(this._1, "s") + ", " + ny_fmt(this._2, "s") + ", " + ny_fmt(this._3, "s") + ", " + ny_fmt(this._4, "s") + ", " + ny_fmt(this._5, "s") + ", " + ny_fmt(this._6, "s") + ")"; }
}
class nyS_Opt_oi {
constructor(p0, p1) { this.has = p0; this.val = p1; }
ny_cp() { return new nyS_Opt_oi(this.has, this.val); }
ny_eq(o) { return this.has === o.has && this.val === o.val; }
ny_fmt() { return this.has ? "Some(" + ny_fmt(this.val, "i") + ")" : "none"; }
}
nyS_Msg.ny_jf = [["tag", "tag", "i"], ["Show_0", "Show_0", "s"]];
nyS_Msg.ny_jk = "e"; nyS_Msg.ny_jn = "Msg"; nyS_Msg.ny_jv = [["Show", [1]]];
nyS_Node.ny_jf = [["html", "html", "s"], ["kit", "kit", ["a", "s"]]];
nyS_WebConfig.ny_jf = [["name", "name", "s"], ["url", "url", "s"], ["lang", "lang", "s"], ["tz", "tz", "s"], ["desc", "desc", "s"], ["share", "share", "s"], ["style", "style", "s"], ["motion", "motion", "s"], ["transition", "transition", "s"], ["font", "font", "s"], ["display", "display", "s"], ["primary", "primary", "s"], ["logo", "logo", "s"], ["say", "say", ["m", "s", "s"]], ["palette", "palette", ["m", "s", "s"]]];
nyS_Opt_of.ny_jf = [["has", "has", "b"], ["val", "val", "f"]];
nyS_Opt_of.ny_jk = "o";
nyS_Tup_t2ss.ny_jf = [["_0", "_0", "s"], ["_1", "_1", "s"]];
nyS_Tup_t2ss.ny_jk = "t";
nyS_Opt_ot2ss.ny_jf = [["has", "has", "b"], ["val", "val", nyS_Tup_t2ss]];
nyS_Opt_ot2ss.ny_jk = "o";
nyS_Tup_t3sss.ny_jf = [["_0", "_0", "s"], ["_1", "_1", "s"], ["_2", "_2", "s"]];
nyS_Tup_t3sss.ny_jk = "t";
nyS_Tup_t7sssssss.ny_jf = [["_0", "_0", "s"], ["_1", "_1", "s"], ["_2", "_2", "s"], ["_3", "_3", "s"], ["_4", "_4", "s"], ["_5", "_5", "s"], ["_6", "_6", "s"]];
nyS_Tup_t7sssssss.ny_jk = "t";
nyS_Opt_oi.ny_jf = [["has", "has", "b"], ["val", "val", "i"]];
nyS_Opt_oi.ny_jk = "o";
function update(m, kind) {
let nyU__ub7_m1, k, ny_t4;
nyU__ub7_m1 = m;
ny_sh(nyU__ub7_m1);
k = nyU__ub7_m1.Show_0;
ny_t4 = k;
kind.v = ny_t4;
}
function roomList(_web, rooms, kind) {
let ny_t3, ny_t4, ny_t5, ny_t6, ny_t7, k, ny_t9, ny_t10, ny_t11, ny_t12, ny_t13, ny_t14, ny_t15, ny_t16, x, ny_t18, r, ny_t20, ny_t21, ny_t22, ny_t23, ny_t24, ny_t25, ny_t26;
ny_t3 = new nyS_Opt_of(false, 0.0);
ny_t4 = [];
ny_t5 = new nyS_Opt_of(false, 0.0);
ny_t6 = [];
ny_t7 = ["All", "Double", "Family", "Suite"];
for (const ny_c1 of ny_t7) {
k = ny_c1;
ny_t9 = new nyS_Msg(0, k);
ny_t10 = nyU_web_u2e__on(ny_t9);
ny_t11 = new nyS_Opt_of(false, 0.0);
ny_t12 = nyU_web_u2e_button(k, "", "", ny_t10, k === kind, "", "", "", "", "", "", "", "", "", ny_t11, "");
{
if (ny_t6.ny_s) ny_t6 = ny_cp(ny_t6);
ny_t6.push(ny_sh(ny_t12));
}
}
ny_t13 = nyU_web_u2e_row("", "", "", "", "", "", "", "", ny_t5, "", ny_t6);
{
if (ny_t4.ny_s) ny_t4 = ny_cp(ny_t4);
ny_t4.push(ny_sh(ny_t13));
}
ny_t14 = new nyS_Opt_of(false, 0.0);
ny_t15 = [];
ny_t16 = [];
for (const ny_c2 of rooms) {
x = ny_c2;
if ((kind === "All") || (x._1 === kind)) {
{
if (ny_t16.ny_s) ny_t16 = ny_cp(ny_t16);
ny_t16.push(ny_sh(x));
}
}
}
ny_t18 = ny_t16;
for (const ny_c3 of ny_t18) {
r = ny_c3;
ny_t20 = `${r._4}`;
ny_t21 = [r._2, r._3];
ny_t22 = new nyS_Opt_of(false, 0.0);
ny_t23 = [];
ny_t24 = nyU_web_u2e_card(r._0, ny_t20, "", r._5, "", false, "", "", "", r._1, ny_t21, "", "", "", "", "", "", "", ny_t22, "", ny_t23, _web);
{
if (ny_t15.ny_s) ny_t15 = ny_cp(ny_t15);
ny_t15.push(ny_sh(ny_t24));
}
}
ny_t25 = nyU_web_u2e_grid(3, "", "", false, "", "", "", "stagger", "zoom", "", "", ny_t14, "", ny_t15);
{
if (ny_t4.ny_s) ny_t4 = ny_cp(ny_t4);
ny_t4.push(ny_sh(ny_t25));
}
ny_t26 = nyU_web_u2e_stack("", "", "", "", "", "", "", "", ny_t3, "", ny_t4);
return ny_t26;
}
function nyU_web_u2e__render(view) {
let ny_t1;
ny_t1 = view.html;
return ny_t1;
}
function nyU_web_u2e_grid(cols, widths, gap, numbers, align, id, cls, anim, hover, loop, scroll, at, morph, kids) {
let c, ny_t15, style, ny_t17, ny_t18, ny_t19, ny_t20, ny_t21, ny_t22, h, ny_t24, ny_t25, ny_t26, ny_t27, ny_t28, ny_t29, ny_t30, ny_t31, ny_t32, ny_t33, ny_t34, ny_t35;
c = "grid";
if (widths !== "") {
{
c += " grid-w";
}
} else if (cols > 0) {
ny_t15 = ` g${cols}`;
{
c += ny_t15;
}
} else {
{
c += " grid-auto";
}
}
if (numbers) {
{
c += " grid-num";
}
}
if (align === "center") {
{
c += " grid-mid";
}
}
style = "";
if (widths !== "") {
ny_t17 = "--widths:" + widths;
ny_t18 = ny_t17 + ";";
{
style += ny_t18;
}
}
if (cols > 6) {
ny_t19 = `--cols:${cols};`;
{
style += ny_t19;
}
{
c += " grid-cols";
}
}
if (gap !== "") {
ny_t20 = nyU_web_u2e__size(gap);
ny_t21 = "--gap:" + ny_t20;
ny_t22 = ny_t21 + ";";
{
style += ny_t22;
}
}
ny_t24 = nyU_web_u2e__ic(c, id, cls);
ny_t25 = "<div" + ny_t24;
ny_t26 = nyU_web_u2e__at("style", style);
ny_t27 = ny_t25 + ny_t26;
ny_t28 = nyU_web_u2e__mo(anim, "", loop, scroll, at, morph);
ny_t29 = ny_t27 + ny_t28;
ny_t30 = ny_t29 + ">";
ny_t31 = nyU_web_u2e__hover_each(kids, hover);
ny_t32 = nyU_web_u2e__html(ny_t31);
ny_t33 = ny_t30 + ny_t32;
h = ny_t33 + "</div>";
ny_t34 = nyU_web_u2e__kits(kids, "");
ny_t35 = new nyS_Node(h, ny_sh(ny_t34));
return ny_t35;
}
function nyU_web_u2e_stack(gap, align, id, cls, anim, hover, loop, scroll, at, morph, kids) {
let ny_t11, ny_t12, ny_t13;
ny_t11 = nyU_web_u2e__mo(anim, "", loop, scroll, at, morph);
ny_t12 = nyU_web_u2e__hover_each(kids, hover);
ny_t13 = nyU_web_u2e__flow("stack", gap, align, id, cls, ny_t11, ny_t12);
return ny_t13;
}
function nyU_web_u2e_row(gap, align, id, cls, anim, hover, loop, scroll, at, morph, kids) {
let ny_t11, ny_t12, ny_t13;
ny_t11 = nyU_web_u2e__mo(anim, "", loop, scroll, at, morph);
ny_t12 = nyU_web_u2e__hover_each(kids, hover);
ny_t13 = nyU_web_u2e__flow("row", gap, align, id, cls, ny_t11, ny_t12);
return ny_t13;
}
function nyU_web_u2e__flow(kind, gap, align, id, cls, motion, kids) {
let c, ny_t8, ny_t9, ny_t10, ny_t11, style, ny_t13, ny_t14, ny_t15, ny_t16, ny_t17, ny_t18, ny_t19, ny_t20, ny_t21, ny_t22, ny_t23, ny_t24, ny_t25;
c = kind;
if (align !== "") {
ny_t8 = " " + kind;
ny_t9 = ny_t8 + "-";
ny_t10 = nyU_web_u2e__word(align);
ny_t11 = ny_t9 + ny_t10;
{
c += ny_t11;
}
}
if (gap !== "") {
ny_t13 = nyU_web_u2e__size(gap);
ny_t14 = "--gap:" + ny_t13;
style = ny_t14 + ";";
} else {
style = "";
}
ny_t15 = nyU_web_u2e__ic(c, id, cls);
ny_t16 = "<div" + ny_t15;
ny_t17 = nyU_web_u2e__at("style", style);
ny_t18 = ny_t16 + ny_t17;
ny_t19 = ny_t18 + motion;
ny_t20 = ny_t19 + ">";
ny_t21 = nyU_web_u2e__html(kids);
ny_t22 = ny_t20 + ny_t21;
ny_t23 = ny_t22 + "</div>";
ny_t24 = nyU_web_u2e__kits(kids, "");
ny_t25 = new nyS_Node(ny_t23, ny_sh(ny_t24));
return ny_t25;
}
function nyU_web_u2e_card(title, text, icon, image, alt, fill, href, on, color, tag, items, ratio, id, cls, anim, hover, loop, scroll, at, morph, kids, _web) {
let c, tag_name, h, ny_t25, ny_t26, ny_t27, t, ny_t29, ny_t30, ny_t31, ny_t32, ny_t33, ny_t34, ny_t35, ny_t36, ny_t37, ny_t38, ny_t39, ny_t40, ny_t41, ny_t42, ny_t43, ny_t44, ny_t45, ny_t46, ny_t47, ny_t48, hit, ny_t50, ny_t51, ny_t52, ny_t53, ny_t54, ny_t55, ny_t56, own, ny_t58, ny_t59, ny_t60, ny_t61, ny_t62, ny_t63, it, ny_t65, ny_t66, ny_t67, ny_t68, ny_t69, ny_t70, ny_t71, ny_t72, ny_t73;
c = "card";
if ((on !== "") && (href === "")) {
{
c += " card-on";
}
}
if (fill) {
{
c += " card-fill";
}
}
if (image !== "") {
{
c += " card-img";
}
}
if ((icon !== "") && (image === "")) {
{
c += " card-feat";
}
}
if (href !== "") {
{
c += " card-link";
}
}
tag_name = (href !== "") ? "a" : "article";
ny_t25 = "<" + tag_name;
ny_t26 = nyU_web_u2e__ic(c, id, cls);
h = ny_t25 + ny_t26;
if (href !== "") {
ny_t27 = nyU_web_u2e__link_attrs(href);
{
h += ny_t27;
}
}
t = nyU_web_u2e__color(color, _web);
if (t !== "") {
ny_t29 = nyU_web_u2e__attr(t);
ny_t30 = " style=\"--tint:" + ny_t29;
ny_t31 = ny_t30 + "\"";
{
h += ny_t31;
}
}
ny_t32 = nyU_web_u2e__mo(anim, hover, loop, scroll, at, morph);
ny_t33 = ny_t32 + ">";
{
h += ny_t33;
}
if (image !== "") {
if (ratio !== "") {
ny_t34 = nyU_web_u2e__attr(ratio);
ny_t35 = " style=\"--ratio:" + ny_t34;
ny_t36 = ny_t35 + "\"";
} else {
ny_t36 = "";
}
ny_t37 = "<div class=\"card-media\"" + ny_t36;
ny_t38 = ny_t37 + ">";
{
h += ny_t38;
}
ny_t39 = nyU_web_u2e__img_tag(image, (alt !== "") ? alt : title, "(min-width: 1024px) 400px, (min-width: 640px) 50vw, 100vw", false);
{
h += ny_t39;
}
if (tag !== "") {
ny_t40 = nyU_web_u2e__md(tag);
ny_t41 = "<span class=\"card-tag\">" + ny_t40;
ny_t42 = ny_t41 + "</span>";
{
h += ny_t42;
}
}
{
h += "</div>";
}
} else if (icon !== "") {
ny_t43 = nyU_web_u2e__icon(icon);
ny_t44 = "<span class=\"card-icon\">" + ny_t43;
ny_t45 = ny_t44 + "</span>";
{
h += ny_t45;
}
}
{
h += "<div class=\"card-body\">";
}
if ((image === "") && (tag !== "")) {
ny_t46 = nyU_web_u2e__md(tag);
ny_t47 = "<span class=\"card-tag\">" + ny_t46;
ny_t48 = ny_t47 + "</span>";
{
h += ny_t48;
}
}
if ((on !== "") && (href === "")) {
ny_t50 = nyU_web_u2e__at("data-on", on);
ny_t51 = "<button type=\"button\" class=\"card-hit\"" + ny_t50;
hit = ny_t51 + ">";
} else {
hit = "";
}
if (title !== "") {
ny_t52 = "<h3 class=\"card-title\">" + hit;
ny_t53 = nyU_web_u2e__md(title);
ny_t54 = ny_t52 + ny_t53;
ny_t55 = ny_t54 + ((hit !== "") ? "</button>" : "");
ny_t56 = ny_t55 + "</h3>";
{
h += ny_t56;
}
}
if (text !== "") {
own = (hit !== "") && (title === "");
ny_t58 = "<p class=\"card-text\">" + (own ? hit : "");
ny_t59 = nyU_web_u2e__md(text);
ny_t60 = ny_t58 + ny_t59;
ny_t61 = ny_t60 + (own ? "</button>" : "");
ny_t62 = ny_t61 + "</p>";
{
h += ny_t62;
}
}
if (items.length > 0) {
{
h += "<ul class=\"card-items\">";
}
ny_t63 = items;
ny_sh(ny_t63);
for (const ny_c1 of ny_t63) {
it = ny_c1;
ny_t65 = nyU_web_u2e__md(it);
ny_t66 = "<li>" + ny_t65;
ny_t67 = ny_t66 + "</li>";
{
h += ny_t67;
}
}
{
h += "</ul>";
}
}
ny_t68 = nyU_web_u2e__html(kids);
ny_t69 = ny_t68 + "</div>";
{
h += ny_t69;
}
if (((icon !== "") && (image === "")) && (!fill)) {
{
h += "<span class=\"card-bar\" aria-hidden=\"true\"></span>";
}
}
ny_t70 = "</" + tag_name;
ny_t71 = ny_t70 + ">";
{
h += ny_t71;
}
ny_t72 = nyU_web_u2e__kits(kids, "");
ny_t73 = new nyS_Node(h, ny_sh(ny_t72));
return ny_t73;
}
function nyU_web_u2e_button(text, href, style, on, active, icon, label, size, id, cls, anim, hover, loop, scroll, at, morph) {
let c, ny_t17, ny_t18, ny_t19, inner, ny_t21, ny_t22, motion, ny_t24, ny_t25, ny_t26, ny_t27, ny_t28, ny_t29, ny_t30, ny_t31, ny_t32, ny_t33, ny_t34, ny_t35, h, ny_t37, ny_t38, ny_t39, ny_t40, ny_t41, ny_t42, ny_t43, ny_t44, ny_t45, ny_t46, ny_t47, ny_t48, ny_t49;
if (style !== "") {
ny_t17 = nyU_web_u2e__word(style);
} else {
ny_t17 = "primary";
}
c = "btn btn-" + ny_t17;
if (size !== "") {
ny_t18 = nyU_web_u2e__word(size);
ny_t19 = " btn-" + ny_t18;
{
c += ny_t19;
}
}
if (active) {
{
c += " is-active";
}
}
if (icon !== "") {
ny_t21 = nyU_web_u2e__icon(icon);
} else {
ny_t21 = "";
}
ny_t22 = nyU_web_u2e__md(text);
inner = ny_t21 + ny_t22;
motion = nyU_web_u2e__mo(anim, hover, loop, scroll, at, morph);
if (href !== "") {
ny_t24 = nyU_web_u2e__ic(c, id, cls);
ny_t25 = "<a" + ny_t24;
ny_t26 = nyU_web_u2e__link_attrs(href);
ny_t27 = ny_t25 + ny_t26;
ny_t28 = nyU_web_u2e__at("aria-label", label);
ny_t29 = ny_t27 + ny_t28;
ny_t30 = ny_t29 + motion;
ny_t31 = ny_t30 + ">";
ny_t32 = ny_t31 + inner;
ny_t33 = ny_t32 + "</a>";
ny_t34 = [];
ny_t35 = new nyS_Node(ny_t33, ny_sh(ny_t34));
return ny_t35;
}
ny_t37 = nyU_web_u2e__ic(c, id, cls);
ny_t38 = "<button type=\"button\"" + ny_t37;
ny_t39 = nyU_web_u2e__at("aria-label", label);
ny_t40 = ny_t38 + ny_t39;
ny_t41 = nyU_web_u2e__at("data-on", on);
h = ny_t40 + ny_t41;
if (on !== "") {
ny_t42 = " aria-pressed=\"" + (active ? "true" : "false");
ny_t43 = ny_t42 + "\"";
{
h += ny_t43;
}
}
ny_t44 = h + motion;
ny_t45 = ny_t44 + ">";
ny_t46 = ny_t45 + inner;
ny_t47 = ny_t46 + "</button>";
ny_t48 = [];
ny_t49 = new nyS_Node(ny_t47, ny_sh(ny_t48));
return ny_t49;
}
function nyU_web_u2e__esc(s) {
let ny_t1, ny_t2, ny_t3, ny_t4, ny_t5;
ny_t1 = ny_str_replace(s, "&", "&amp;", 1, 5);
ny_t2 = ny_str_replace(ny_t1, "<", "&lt;", 1, 5);
ny_t3 = ny_str_replace(ny_t2, ">", "&gt;", 1, 5);
ny_t4 = ny_str_replace(ny_t3, "\"", "&quot;", 1, 5);
ny_t5 = ny_str_replace(ny_t4, "'", "&#39;", 1, 5);
return ny_t5;
}
function nyU_web_u2e__attr(s) {
let ny_t1;
ny_t1 = nyU_web_u2e__esc(s);
return ny_t1;
}
function nyU_web_u2e__at(name, value) {
let ny_t2, ny_t3, ny_t4, ny_t5, ny_t6;
if (value === "") {
ny_t6 = "";
} else {
ny_t2 = " " + name;
ny_t3 = ny_t2 + "=\"";
ny_t4 = nyU_web_u2e__attr(value);
ny_t5 = ny_t3 + ny_t4;
ny_t6 = ny_t5 + "\"";
}
return ny_t6;
}
function nyU_web_u2e__md(s) {
let cs, n, o, i, bold, em, c, ny_t8, ny_t9, ny_t10, ny_t11, ny_t12, ny_t13, ny_t14, ny_t15, ny_t16, ny_t17, ny_t18, ny_t19, ny_t20, ny_t21, ny_t22, ny_t23, ny_t24, ny_t25, ny_t26, ny_t27, ny_t28, ny_t29, ny_t30, ny_t31, ny_t32, ny_t33, j, ny_t35, ny_t36, ny_t37, ny_t38, ny_t39, ny_t40, ny_t41, ny_t42, mid, ny_t44, end, url, ny_t47, ny_t48, ny_t49, ny_t50, ny_t51, ny_t52, ny_t53, ny_t54, ny_t55, ny_t56, ny_t57, ny_j_2, name, ny_t60, ny_t61, ny_t62, ny_t63, ny_t64, ny_t65, ny_t66, ny_t67, ny_t68, ny_t69, ny_t70, ny_t71, ny_t72;
cs = ny_chars(s);
n = cs.length;
o = "";
i = 0;
bold = false;
em = false;
while (i < n) {
c = ny_get(cs, i, 1, 5);
if ((c === 92) && ((i + 1) < n)) {
ny_t8 = ny_get(cs, i + 1, 1, 5);
ny_t9 = `${ny_char_str(ny_t8)}`;
ny_t10 = nyU_web_u2e__esc(ny_t9);
{
o += ny_t10;
}
i = i + 2;
} else {
ny_t12 = c === 42;
if (ny_t12) {
ny_t11 = ny_add(i, 1, 1, 5);
ny_t12 = ny_t11 < n;
}
ny_t15 = ny_t12;
if (ny_t15) {
ny_t13 = ny_add(i, 1, 1, 5);
ny_t14 = ny_get(cs, ny_t13, 1, 5);
ny_t15 = ny_t14 === 42;
}
ny_t20 = ny_t15;
if (ny_t20) {
ny_t19 = bold;
if (!ny_t19) {
ny_t16 = ny_add(i, 2, 1, 5);
ny_t17 = nyU_web_u2e__find(cs, ny_t16, "**");
ny_t18 = ny_add(i, 2, 1, 5);
ny_t19 = ny_t17 > ny_t18;
}
ny_t20 = ny_t19;
}
if (ny_t20) {
{
o += bold ? "</strong>" : "<strong>";
}
bold = !bold;
i = ny_add(i, 2, 1, 5);
} else {
ny_t30 = c === 42;
if (ny_t30) {
ny_t29 = em;
if (!ny_t29) {
ny_t21 = ny_add(i, 1, 1, 5);
ny_t24 = ny_t21 < n;
if (ny_t24) {
ny_t22 = ny_add(i, 1, 1, 5);
ny_t23 = ny_get(cs, ny_t22, 1, 5);
ny_t24 = ny_t23 !== 32;
}
ny_t28 = ny_t24;
if (ny_t28) {
ny_t25 = ny_add(i, 1, 1, 5);
ny_t26 = nyU_web_u2e__find(cs, ny_t25, "*");
ny_t27 = ny_add(i, 1, 1, 5);
ny_t28 = ny_t26 > ny_t27;
}
ny_t29 = ny_t28;
}
ny_t30 = ny_t29;
}
if (ny_t30) {
{
o += em ? "</em>" : "<em>";
}
em = !em;
i = ny_add(i, 1, 1, 5);
} else {
ny_t33 = c === 96;
if (ny_t33) {
ny_t31 = ny_add(i, 1, 1, 5);
ny_t32 = nyU_web_u2e__find(cs, ny_t31, "`");
                        ny_t33 = ny_t32 > i;
                    }
                    if (ny_t33) {
                        ny_t35 = ny_add(i, 1, 1, 5);
                        j = nyU_web_u2e__find(cs, ny_t35, "`");
ny_t36 = ny_add(i, 1, 1, 5);
ny_t37 = ny_str_slice(s, ny_t36, j, 1, 5);
ny_t38 = nyU_web_u2e__esc(ny_t37);
ny_t39 = "<code>" + ny_t38;
ny_t40 = ny_t39 + "</code>";
{
o += ny_t40;
}
i = ny_add(j, 1, 1, 5);
} else {
ny_t42 = c === 91;
if (ny_t42) {
ny_t41 = nyU_web_u2e__link_end(cs, i);
ny_t42 = ny_t41 > i;
}
if (ny_t42) {
ny_t44 = ny_add(i, 1, 1, 5);
mid = nyU_web_u2e__find(cs, ny_t44, "](");
end = nyU_web_u2e__link_end(cs, i);
ny_t47 = ny_add(mid, 2, 1, 5);
url = ny_str_slice(s, ny_t47, end, 1, 5);
ny_t48 = nyU_web_u2e__link_attrs(url);
ny_t49 = "<a" + ny_t48;
ny_t50 = ny_t49 + ">";
ny_t51 = ny_add(i, 1, 1, 5);
ny_t52 = ny_str_slice(s, ny_t51, mid, 1, 5);
ny_t53 = nyU_web_u2e__md(ny_t52);
ny_t54 = ny_t50 + ny_t53;
ny_t55 = ny_t54 + "</a>";
{
o += ny_t55;
}
i = ny_add(end, 1, 1, 5);
} else {
ny_t57 = c === 58;
if (ny_t57) {
ny_t56 = nyU_web_u2e__icon_end(cs, i);
ny_t57 = ny_t56 > i;
}
if (ny_t57) {
ny_j_2 = nyU_web_u2e__icon_end(cs, i);
ny_t60 = ny_add(i, 1, 1, 5);
name = ny_str_slice(s, ny_t60, ny_j_2, 1, 5);
ny_t61 = nyU_web_u2e__icon_kind(name);
if (ny_t61 > 0) {
ny_t62 = nyU_web_u2e__icon(name);
{
o += ny_t62;
}
i = ny_add(ny_j_2, 1, 1, 5);
} else {
ny_t63 = nyU_web_u2e__icon_like(name);
ny_t64 = ny_t63 !== "";
if (ny_t64) {
ny_t65 = "text " + name;
nyU_web_u2e__warn("E0389", ny_t65);
ny_t66 = nyU_web_u2e__icon_like(name);
ny_t67 = nyU_web_u2e__icon(ny_t66);
{
o += ny_t67;
}
i = ny_add(ny_j_2, 1, 1, 5);
} else {
ny_t68 = "text " + name;
nyU_web_u2e__warn("E0389", ny_t68);
ny_t69 = `${ny_char_str(c)}`;
ny_t70 = nyU_web_u2e__esc(ny_t69);
{
o += ny_t70;
}
i = ny_add(i, 1, 1, 5);
}
}
} else {
ny_t71 = `${ny_char_str(c)}`;
ny_t72 = nyU_web_u2e__esc(ny_t71);
{
o += ny_t72;
}
i = ny_add(i, 1, 1, 5);
}
}
}
}
}
}
}
if (em) {
{
o += "</em>";
}
}
if (bold) {
{
o += "</strong>";
}
}
return o;
}
function nyU_web_u2e__find(cs, from, pat) {
let ps, i, ny_t5, ok, k, ny_t8, ny_t9, ny_t10, ny_t11, ny_t12, ny_t13;
ps = ny_chars(pat);
i = from;
for (;; ) {
ny_t5 = ny_add(i, ps.length, 1, 5);
if (!(ny_t5 <= cs.length)) break;
ok = true;
k = 0;
ny_t8 = ps.length;
for (; k < ny_t8; k = k + 1) {
ny_t9 = ny_add(i, k, 1, 5);
ny_t10 = ny_get(cs, ny_t9, 1, 5);
ny_t11 = ny_get(ps, k, 1, 5);
if (ny_t10 !== ny_t11) {
ok = false;
break;
}
}
if (ok) {
ny_t12 = i;
return ny_t12;
}
i = ny_add(i, 1, 1, 5);
}
ny_t13 = -1 + 0;
return ny_t13;
}
function nyU_web_u2e__link_end(cs, at) {
let mid, ny_t3, depth, k, c;
ny_t3 = ny_add(at, 1, 1, 5);
mid = nyU_web_u2e__find(cs, ny_t3, "](");
if (mid < 0) {
return -1 + 0;
}
depth = 0;
k = ny_add(mid, 2, 1, 5);
while (k < cs.length) {
c = ny_get(cs, k, 1, 5);
if (c === 10) {
return -1 + 0;
}
if (c === 40) {
depth = depth + 1;
} else if (c === 41) {
if (depth === 0) {
return k;
}
depth = depth - 1;
}
k = k + 1;
}
return -1 + 0;
}
function nyU_web_u2e__icon_end(cs, at) {
let ny_t2, ny_t3, ny_t4, ny_t5, ny_t6, j, ny_t8, ny_t9, ny_t10, ny_t11, ny_t12, ny_t13, ny_t14, ny_t15, ny_t16;
ny_t2 = ny_add(at, 1, 1, 5);
ny_t6 = ny_t2 >= cs.length;
if (!ny_t6) {
ny_t3 = ny_add(at, 1, 1, 5);
ny_t4 = ny_get(cs, ny_t3, 1, 5);
ny_t5 = nyU_web_u2e__lower(ny_t4);
ny_t6 = !ny_t5;
}
if (ny_t6) {
return -1 + 0;
}
j = ny_add(at, 1, 1, 5);
for (;; ) {
ny_t14 = j < cs.length;
if (ny_t14) {
ny_t8 = ny_get(cs, j, 1, 5);
ny_t9 = nyU_web_u2e__lower(ny_t8);
ny_t11 = ny_t9;
if (!ny_t11) {
ny_t10 = ny_get(cs, j, 1, 5);
ny_t11 = ny_char_is_digit(ny_t10);
}
ny_t13 = ny_t11;
if (!ny_t13) {
ny_t12 = ny_get(cs, j, 1, 5);
ny_t13 = ny_t12 === 45;
}
ny_t14 = ny_t13;
}
if (!ny_t14) break;
j = ny_add(j, 1, 1, 5);
}
ny_t16 = j < cs.length;
if (ny_t16) {
ny_t15 = ny_get(cs, j, 1, 5);
ny_t16 = ny_t15 === 58;
}
if (ny_t16) {
return j;
}
return -1 + 0;
}
function nyU_web_u2e__lower(c) {
return (c >= 97) && (c <= 122);
}
function nyU_web_u2e__icon(name) {
let n, ny_t2, ny_t3, ny_t4, ny_t5, brand, ny_t7, ny_t8, ny_t9, ny_t10, ny_t11, ny_t12, ny_t13;
n = name;
ny_t2 = nyU_web_u2e__icon_kind(n);
if (ny_t2 === 0) {
ny_t3 = "icon " + name;
nyU_web_u2e__warn("E0389", ny_t3);
ny_t4 = nyU_web_u2e__icon_like(name);
if (ny_t4 !== "") {
ny_t5 = nyU_web_u2e__icon_like(name);
} else {
ny_t5 = "circle";
}
n = ny_t5;
}
ny_t7 = nyU_web_u2e__icon_kind(n);
brand = (ny_t7 === 2) ? " i-brand" : "";
ny_t8 = "<svg class=\"i" + brand;
ny_t9 = ny_t8 + " i-";
ny_t10 = ny_t9 + n;
ny_t11 = ny_t10 + "\" viewBox=\"0 0 24 24\" aria-hidden=\"true\"><use href=\"#i-";
ny_t12 = ny_t11 + n;
ny_t13 = ny_t12 + "\"/></svg>";
return ny_t13;
}
function nyU_web_u2e__href(url) {
let u, scheme, ny_t3, ny_t4, ny_t5, ny_t6, ny_t7, ny_t8, ny_t9, ny_t10;
u = ny_str_trim(url);
scheme = nyU_web_u2e__scheme(u);
if (((((scheme !== "") && (scheme !== "http")) && (scheme !== "https")) && (scheme !== "mailto")) && (scheme !== "tel")) {
ny_t3 = "the link `" + u;
        ny_t4 = ny_t3 + "` uses the scheme `";
        ny_t5 = ny_t4 + scheme;
        ny_t6 = ny_t5 + ":`, which a page may not link to";
nyU_web_u2e__err("E0381", ny_t6);
ny_t7 = "#";
return ny_t7;
}
if (scheme === "tel") {
ny_t8 = ny_str_replace(u, " ", "", 1, 5);
ny_t9 = nyU_web_u2e__attr(ny_t8);
return ny_t9;
}
ny_t10 = nyU_web_u2e__attr(u);
return ny_t10;
}
function nyU_web_u2e__link_attrs(url) {
let u, ext, ny_t3, ny_t4, ny_t5, ny_t6, ny_t7, ny_t8, ny_t9;
u = ny_str_trim(url);
ny_t3 = ny_str_lower(u);
ny_t5 = ny_t3.startsWith("http://");
if (!ny_t5) {
ny_t4 = ny_str_lower(u);
ny_t5 = ny_t4.startsWith("https://");
}
ext = ny_t5;
ny_t6 = nyU_web_u2e__href(u);
ny_t7 = " href=\"" + ny_t6;
ny_t8 = ny_t7 + "\"";
ny_t9 = ny_t8 + (ext ? " target=\"_blank\" rel=\"noopener noreferrer\"" : "");
return ny_t9;
}
function nyU_web_u2e__scheme(u) {
let cs, i, c, ny_t4, ny_t5, ny_t6, ny_t7;
cs = ny_chars(u);
i = 0;
while (i < cs.length) {
c = ny_get(cs, i, 1, 5);
if (c === 58) {
if (i > 0) {
ny_t4 = ny_str_slice(u, 0, i, 1, 5);
ny_t5 = ny_str_lower(ny_t4);
} else {
ny_t5 = "";
}
return ny_t5;
}
if (!(ny_char_is_letter(c) || ((i > 0) && (((ny_char_is_digit(c) || (c === 43)) || (c === 45)) || (c === 46))))) {
ny_t6 = "";
return ny_t6;
}
i = i + 1;
}
ny_t7 = "";
return ny_t7;
}
function nyU_web_u2e__src(src) {
let s, scheme, ny_t3, ny_t4, ny_t5, ny_t6, ny_t7;
s = ny_str_trim(src);
scheme = nyU_web_u2e__scheme(s);
if (((scheme === "http") || (scheme === "https")) || s.startsWith("/")) {
return s;
}
if (scheme !== "") {
ny_t3 = "the image `" + s;
        ny_t4 = ny_t3 + "` uses the scheme `";
        ny_t5 = ny_t4 + scheme;
        ny_t6 = ny_t5 + ":`: use a file of `assets/` or an https URL";
nyU_web_u2e__err("E0381", ny_t6);
ny_t7 = "";
return ny_t7;
}
return s;
}
function nyU_web_u2e__img_tag(src, alt, sizes, first) {
let s, ny_t5, ny_t6, ny_t7, ny_t8, ny_t9, ny_t10, ny_t11, ny_t12, ny_t13, ny_t14, ny_t15, ny_t16, ny_t17, ny_t18, ny_t19, ny_t20;
s = nyU_web_u2e__src(src);
if (first) {
ny_t5 = nyU_web_u2e__attr(s);
ny_t6 = "<img fetchpriority=\"high\" src=\"" + ny_t5;
ny_t7 = ny_t6 + "\" alt=\"";
ny_t8 = nyU_web_u2e__attr(alt);
ny_t9 = ny_t7 + ny_t8;
ny_t10 = ny_t9 + "\" decoding=\"async\" sizes=\"";
ny_t11 = ny_t10 + sizes;
ny_t12 = ny_t11 + "\">";
return ny_t12;
}
ny_t13 = nyU_web_u2e__attr(s);
ny_t14 = "<img src=\"" + ny_t13;
ny_t15 = ny_t14 + "\" alt=\"";
ny_t16 = nyU_web_u2e__attr(alt);
ny_t17 = ny_t15 + ny_t16;
ny_t18 = ny_t17 + "\" loading=\"lazy\" decoding=\"async\" sizes=\"";
ny_t19 = ny_t18 + sizes;
ny_t20 = ny_t19 + "\">";
return ny_t20;
}
function nyU_web_u2e__err(code, msg) {
let ny_t2, ny_t3, ny_t4, ny_t5, ny_t6, ny_t7;
ny_t2 = "/*! nyra:error " + code;
ny_t3 = ny_t2 + " ";
ny_t4 = ny_str_replace(msg, "*/", "* /", 1, 5);
ny_t5 = ny_str_replace(ny_t4, "\n", " ", 1, 5);
ny_t6 = ny_t3 + ny_t5;
ny_t7 = ny_t6 + " */";
ny_std_web__css(ny_t7, 1, 5);
}
function nyU_web_u2e__warn(code, what) {
let ny_t2, ny_t3, ny_t4, ny_t5, ny_t6, ny_t7;
ny_t2 = "/*! nyra:warn " + code;
ny_t3 = ny_t2 + " ";
ny_t4 = ny_str_replace(what, "*/", "* /", 1, 5);
ny_t5 = ny_str_replace(ny_t4, "\n", " ", 1, 5);
ny_t6 = ny_t3 + ny_t5;
ny_t7 = ny_t6 + " */";
ny_std_web__css(ny_t7, 1, 5);
}
function nyU_web_u2e__color(c, _web) {
let ny_t2, ny_t3, ny_t4, ny_t5, ny_t6, ny_t7, ny_t8, ny_t9, ny_t10;
if (((((c === "") || c.startsWith("#")) || c.startsWith("rgb")) || c.startsWith("hsl")) || c.startsWith("oklch")) {
ny_t2 = c;
return ny_t2;
}
if ((((((c === "primary") || (c === "accent")) || (c === "bg")) || (c === "alt")) || (c === "ink")) || (c === "muted")) {
ny_t3 = "var(--" + c;
ny_t4 = ny_t3 + ")";
return ny_t4;
}
if (_web.palette.has(ny_ik(c))) {
ny_t5 = "var(--c-" + c;
ny_t6 = ny_t5 + ")";
return ny_t6;
}
if ((c === "white") || (c === "black")) {
ny_t7 = c;
return ny_t7;
}
ny_t8 = "unknown color `" + c;
    ny_t9 = ny_t8 + "`: use a hex color, a theme color (primary, accent, bg, alt, ink, muted) or a name of `site(palette:)`";
nyU_web_u2e__err("E0374", ny_t9);
ny_t10 = "";
return ny_t10;
}
function nyU_web_u2e__css_value(v) {
let ny_t1, ny_t2, ny_t3, ny_t4;
ny_t1 = ny_str_replace(v, ";", "", 1, 5);
ny_t2 = ny_str_replace(ny_t1, "}", "", 1, 5);
ny_t3 = ny_str_replace(ny_t2, "{", "", 1, 5);
ny_t4 = ny_str_replace(ny_t3, "<", "", 1, 5);
return ny_t4;
}
function nyU_web_u2e__word(w) {
let o, ny_t2, ny_t3, c, ny_t5, ny_t6;
o = "";
ny_t2 = ny_str_lower(w);
ny_t3 = ny_chars(ny_t2);
for (const ny_c1 of ny_t3) {
c = ny_c1;
ny_t5 = nyU_web_u2e__lower(c);
if ((ny_t5 || ny_char_is_digit(c)) || (c === 45)) {
ny_t6 = `${ny_char_str(c)}`;
{
o += ny_t6;
}
}
}
return o;
}
function nyU_web_u2e__size(s) {
let ny_t1, ny_t2, ny_t3, ny_t4, ny_t5, ny_t6;
ny_t1 = ny_std_text_is_float(s, 1, 5);
if (ny_t1) {
ny_t3 = ny_str_to_float(s, 1, 5);
ny_t2 = new nyS_Opt_of(true, ny_t3);
} else {
ny_t2 = new nyS_Opt_of(false, 0.0);
}
ny_t4 = new nyS_Opt_of(false, 0.0);
if (!ny_eq(ny_t2, ny_t4)) {
ny_t6 = s + "rem";
} else {
ny_t5 = nyU_web_u2e__css_value(s);
ny_t6 = ny_str_replace(ny_t5, "\"", "", 1, 5);
}
return ny_t6;
}
function nyU_web_u2e__join(a, b) {
let ny_t2, ny_t3;
if (b === "") {
ny_t3 = a;
} else {
ny_t2 = a + " ";
ny_t3 = ny_t2 + b;
}
return ny_t3;
}
function nyU_web_u2e__ic(base, id, cls) {
let ny_t3, ny_t4, ny_t5, ny_t6, ny_t7, ny_t8;
ny_t3 = nyU_web_u2e__at("id", id);
ny_t4 = ny_t3 + " class=\"";
ny_t5 = nyU_web_u2e__join(base, cls);
ny_t6 = nyU_web_u2e__attr(ny_t5);
ny_t7 = ny_t4 + ny_t6;
ny_t8 = ny_t7 + "\"";
return ny_t8;
}
function nyU_web_u2e__mo(anim, hover, loop, scroll, at, morph) {
let o, ny_t7, ny_t8, ny_t9, ny_t10, ny_t11, ny_t12, nyU__ub7_o4, t, ny_t15, ny_t16, ny_t17;
ny_t7 = nyU_web_u2e__at("data-anim", anim);
ny_t8 = nyU_web_u2e__at("data-hover", hover);
ny_t9 = ny_t7 + ny_t8;
ny_t10 = nyU_web_u2e__at("data-loop", loop);
ny_t11 = ny_t9 + ny_t10;
ny_t12 = nyU_web_u2e__at("data-scroll", scroll);
o = ny_t11 + ny_t12;
nyU__ub7_o4 = ny_sh(at);
if (nyU__ub7_o4.has) {
t = nyU__ub7_o4.val;
ny_t15 = ` data-at="${t}"`;
{
o += ny_t15;
}
}
ny_t16 = nyU_web_u2e__at("data-morph", morph);
ny_t17 = o + ny_t16;
return ny_t17;
}
function nyU_web_u2e__hover_each(kids, hover) {
let ny_t2, out, ny_t4, k, ny_t6, ny_t7;
if (hover === "") {
ny_t2 = kids;
ny_sh(ny_t2);
return ny_t2;
}
out = [];
ny_t4 = kids;
ny_sh(ny_t4);
for (const ny_c1 of ny_t4) {
k = ny_c1;
ny_t6 = nyU_web_u2e__first_attr(k.html, "data-hover", hover);
ny_t7 = new nyS_Node(ny_t6, ny_sh(k.kit));
{
if (out.ny_s) out = ny_cp(out);
out.push(ny_sh(ny_t7));
}
}
return out;
}
function nyU_web_u2e__first_attr(html, name, value) {
let ny_t3, close, ny_t5, ny_t6, ny_t7, ny_t8, ny_t9, end, ny_t11, ny_t12, ny_t13, ny_t14, ny_t15, ny_t16, ny_t17;
if (!html.startsWith("<")) {
ny_t3 = html;
return ny_t3;
}
close = ny_str_index_of(html, ">");
ny_t8 = close < 0;
if (!ny_t8) {
ny_t5 = ny_str_slice(html, 0, close, 1, 5);
ny_t6 = " " + name;
ny_t7 = ny_t6 + "=";
ny_t8 = ny_t5.includes(ny_t7);
}
if (ny_t8) {
ny_t9 = html;
return ny_t9;
}
end = 1;
for (;; ) {
ny_t12 = end < close;
if (ny_t12) {
ny_t11 = ny_str_slice(html, end, end + 1, 1, 5);
ny_t12 = !" \n\t/".includes(ny_t11);
}
if (!ny_t12) break;
end = end + 1;
}
ny_t13 = ny_str_slice(html, 0, end, 1, 5);
ny_t14 = nyU_web_u2e__at(name, value);
ny_t15 = ny_t13 + ny_t14;
ny_t16 = ny_str_slice(html, end, ny_len(html), 1, 5);
ny_t17 = ny_t15 + ny_t16;
return ny_t17;
}
function nyU_web_u2e__html(kids) {
let h, ny_t2, k;
h = "";
ny_t2 = kids;
ny_sh(ny_t2);
for (const ny_c1 of ny_t2) {
k = ny_c1;
{
h += k.html;
}
}
return h;
}
function nyU_web_u2e__kits(kids, own) {
let all, ny_t3, ny_t4, k, ny_t6, c, ny_t8;
all = [];
if (own !== "") {
ny_t3 = own;
{
if (all.ny_s) all = ny_cp(all);
all.push(ny_t3);
}
}
ny_t4 = kids;
ny_sh(ny_t4);
for (const ny_c1 of ny_t4) {
k = ny_c1;
ny_t6 = k.kit;
ny_sh(ny_t6);
for (const ny_c2 of ny_t6) {
c = ny_c2;
if (!(ny_aindex(all, c) >= 0)) {
ny_t8 = c;
{
if (all.ny_s) all = ny_cp(all);
all.push(ny_t8);
}
}
}
}
return all;
}
function nyU_web_u2e__icon_kind(name) {
let icons, brands, key, ny_t4, ny_t5;
icons = "|accessibility|activity|alert-circle|alert-triangle|apple|arrow-down|arrow-left|arrow-right|arrow-up|arrow-up-right|at-sign|award|axe|baby|badge-check|banknote|bar-chart|bar-chart-2|bar-chart-3|beer|bell|bike|book-open|bookmark|brick-wall|briefcase|building|building-2|bus|cake|cake-slice|calendar|calendar-days|camera|candy|car|carrot|cat|chart-column|chart-line|check|check-check|check-circle|check-circle-2|chef-hat|cherry|chevron-down|chevron-left|chevron-right|chevron-up|chevrons-down|church|circle|circle-alert|circle-check|circle-help|circle-x|clipboard-list|clock|cloud|cloud-sun|cloud-upload|code|coffee|coins|compass|construction|cookie|cpu|credit-card|croissant|crown|cup-soda|database|dog|dollar-sign|donut|download|drill|droplet|droplets|dumbbell|egg|ellipsis|euro|expand|external-link|eye|factory|file-text|film|flag|flame|flower|flower-2|fuel|gem|gift|globe|graduation-cap|grid|grid-3x3|hammer|hand-heart|handshake|hard-hat|headphones|heart|help-circle|history|home|hotel|hourglass|house|ice-cream-cone|image|images|info|key|landmark|languages|laptop|layers|layout-grid|leaf|lightbulb|line-chart|link|link-2|list-checks|lock|mail|mail-open|map|map-pin|maximize-2|medal|menu|message-circle|message-square|mic|minimize-2|minus|monitor|moon|more-horizontal|mountain|move-right|music|navigation|package|paint-roller|paintbrush|palette|party-popper|pen-tool|pencil|percent|phone|phone-call|piggy-bank|pill|pizza|plane|play|plus|printer|quote|receipt|recycle|rocket|ruler|sandwich|scale|school|scissors|search|send|server|settings|share-2|shield|shield-check|ship|shopping-bag|shopping-cart|shovel|sliders-horizontal|smartphone|smile|snowflake|sparkles|sprout|star|stethoscope|store|sun|syringe|tag|target|tent|terminal|thermometer|thumbs-up|timer|train|train-front|tree-pine|trees|trending-up|triangle-alert|trophy|truck|tv|umbrella|upload|user|users|utensils|verified|video|wallet|warehouse|waves|wheat|wifi|wind|wine|wrench|x|x-circle|zap|zoom-in|";
brands = "|facebook|";
ny_t4 = "|" + name;
key = ny_t4 + "|";
ny_t5 = icons.includes(key) ? 1 : (brands.includes(key) ? 2 : 0);
return ny_t5;
}
function nyU_web_u2e__icon_like(name) {
let pairs, at, ny_t3, ny_t4, ny_t5, rest, ny_t7;
pairs = "|access-time=clock|account=user|airplane=plane|alert=triangle-alert|apple-alt=apple|apps=layout-grid|arrow-trend-up=trending-up|at=at-sign|bag=shopping-bag|balance-scale=scale|bars=menu|basket=shopping-bag|bell-o=bell|bicycle=bike|birthday-cake=cake|boat=ship|bolt=zap|build=wrench|bulb=lightbulb|bullseye=target|bus-alt=bus|calendar-alt=calendar|calendar-o=calendar|calendar-week=calendar-days|call=phone|camera-alt=camera|campground=tent|candy-cane=candy|capsules=pill|car-side=car|cart=shopping-cart|cart-shopping=shopping-cart|cash=banknote|cellphone=smartphone|chart-bar=chart-column|chat=message-circle|check-circle-o=circle-check|check-square=check|checkmark=check|circle-check-big=circle-check|circle-exclamation=circle-alert|circle-info=info|circle-question=circle-help|city=building-2|clock-o=clock|close=x|cloud-upload-alt=cloud-upload|cog=settings|cogs=settings|comment=message-circle|comments=message-square|computer=monitor|cookie-bite=cookie|credit-card-alt=credit-card|cross=x|cut=scissors|cutlery=utensils|date=calendar|delivery=truck|description=file-text|desktop=monitor|diamond=gem|directions=navigation|document=file-text|dollar=dollar-sign|done=check|drop=droplet|e-mail=mail|edit=pencil|ellipsis-h=ellipsis|email=mail|envelope=mail|envelope-o=mail|envelope-open=mail-open|error=circle-alert|euro-sign=euro|event=calendar|exclamation-circle=circle-alert|exclamation-triangle=triangle-alert|external-link-alt=external-link|eye-open=eye|face-smile=smile|facebook-f=facebook|facebook-square=facebook|file-alt=file-text|fire=flame|flash=zap|flight=plane|fork-knife=utensils|gas-pump=fuel|gear=settings|globe-alt=globe|grid-alt=layout-grid|group=users|hamburger=menu|hand-holding-heart=hand-heart|headset=headphones|heart-o=heart|heart-pulse=activity|heartbeat=activity|help=circle-help|history-alt=history|home-alt=house|hourglass-half=hourglass|house-chimney=house|ice-cream=ice-cream-cone|idea=lightbulb|info-circle=info|language=languages|lightning=zap|like=thumbs-up|link-alt=link|list-check=list-checks|local-shipping=truck|location=map-pin|location-arrow=navigation|location-dot=map-pin|location-on=map-pin|location-pin=map-pin|magnifier=search|magnifying-glass=search|mail-alt=mail|map-alt=map|map-marker=map-pin|map-marker-alt=map-pin|message=message-square|microchip=cpu|mobile=smartphone|mobile-alt=smartphone|mobile-phone=smartphone|money=banknote|money-bill=banknote|moon-o=moon|more=ellipsis|more-horiz=ellipsis|music-note=music|navicon=menu|notifications=bell|office=building|paint-brush=paintbrush|paper-plane=send|payment=credit-card|pen=pencil|pencil-alt=pencil|people=users|percentage=percent|person=user|phone-alt=phone|phone-square=phone|photo=image|picture=image|pills=pill|pizza-slice=pizza|place=map-pin|plane-departure=plane|print=printer|question=circle-help|question-circle=circle-help|quote-left=quote|quote-right=quote|restaurant=utensils|schedule=clock|seedling=sprout|share-alt=share-2|shipping=truck|shipping-fast=truck|shop=store|sliders=sliders-horizontal|sliders-h=sliders-horizontal|smile-o=smile|star-o=star|stopwatch=timer|store-alt=store|sun-o=sun|tags=tag|task-alt=circle-check|team=users|telephone=phone|th=layout-grid|th-large=layout-grid|thermometer-half=thermometer|thumbs-o-up=thumbs-up|tick=check|time=clock|times=x|tint=droplet|tools=wrench|translate=languages|trending=trending-up|triangle-exclamation=triangle-alert|trophy-alt=trophy|universal-access=accessibility|university=school|user-alt=user|user-friends=users|user-o=user|users-alt=users|video-camera=video|visibility=eye|warning=triangle-alert|water=droplet|web=globe|website=globe|wheelchair=accessibility|wine-glass=wine|world=globe|xmark=x|";
ny_t3 = "|" + name;
ny_t4 = ny_t3 + "=";
at = ny_str_index_of(pairs, ny_t4);
if (at < 0) {
ny_t5 = "";
return ny_t5;
}
rest = ny_str_slice(pairs, (at + ny_len(name)) + 2, ny_len(pairs), 1, 5);
ny_t7 = ny_str_slice(rest, 0, ny_str_index_of(rest, "|"), 1, 5);
return ny_t7;
}
function nyU_web_u2e__on(m) {
let ny_t1;
ny_t1 = ny_jenc(m, nyS_Msg);
return ny_t1;
}
const ny_st = {
"kind": ny_from("All", "s"),
"_web": ny_from({"name":"Penzion U Mlýna","url":"","lang":"en","tz":"Europe/Prague","desc":"","share":"","style":"soft","motion":"","transition":"","font":"Nunito Sans","display":"Fraunces","primary":"#7A4E2D","logo":"","say":{},"palette":{}}, nyS_WebConfig),
"rooms": ny_from([["River Room","Double","18 m² · 2 guests","2,400 Kč per night","A window seat right above the weir.","https://picsum.photos/seed/mlyn-room-1/800/600","The River Room with a window seat"],["Garden Room","Double","16 m² · 2 guests","2,100 Kč per night","Opens onto the herb garden and is quiet at night.","https://picsum.photos/seed/mlyn-room-2/800/600","The Garden Room with a view of the herb garden"],["Miller's Family Room","Family","32 m² · 4 guests","3,600 Kč per night","A double bed and a bunk bed under old oak beams.","https://picsum.photos/seed/mlyn-room-3/800/600","The family room with a bunk bed"],["Attic Family Room","Family","36 m² · 5 guests","3,900 Kč per night","Two bedrooms under the roof, ideal with older kids.","https://picsum.photos/seed/mlyn-room-4/800/600","The attic family room under the roof"],["Castle View Suite","Suite","45 m² · 2 guests","5,200 Kč per night","A free-standing bath and the best view of the castle tower.","https://picsum.photos/seed/mlyn-room-5/800/600","The Castle View Suite with a free-standing bath"],["Mill Wheel Suite","Suite","50 m² · 4 guests","5,900 Kč per night","A living room with the original mill wheel behind glass.","https://picsum.photos/seed/mlyn-room-6/800/600","The Mill Wheel Suite with the old mill wheel"]], ["a", nyS_Tup_t7sssssss]),
};
const ny_web = {
update(text) {
const ny_m = ny_from(JSON.parse(text), nyS_Msg);
const ny_b0 = {v: ny_st["kind"]};
update(ny_m, ny_b0);
ny_st["kind"] = ny_b0.v;
},
views: {
"roomList": () => {
const ny_r = nyU_web_u2e__render(roomList(ny_st["_web"], ny_st["rooms"], ny_st["kind"]));
return ny_r;
},
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
