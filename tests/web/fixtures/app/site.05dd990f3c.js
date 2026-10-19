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
const select = (box, tab, focus) => {
for (const t of box.querySelectorAll(':scope > .tabs > a')) {
const on = t === tab;
t.setAttribute('aria-selected', String(on));
t.tabIndex = on ? 0 : -1;
const p = document.getElementById(t.getAttribute('href').slice(1));
if (p) p.hidden = !on;
}
if (focus) tab.focus();
};
for (const box of document.querySelectorAll('.tabbed')) {
const nav = box.querySelector(':scope > .tabs');
const tabs = nav ? [...nav.querySelectorAll(':scope > a')] : [];
if (!tabs.length) continue;
nav.setAttribute('role', 'tablist');
for (const t of tabs) {
const id = t.getAttribute('href').slice(1);
t.setAttribute('role', 'tab');
t.id = id + '-tab';
t.setAttribute('aria-controls', id);
t.removeAttribute('aria-current');
const p = document.getElementById(id);
if (p) {
p.setAttribute('role', 'tabpanel');
p.setAttribute('aria-labelledby', t.id);
p.tabIndex = 0;
}
}
box.dataset.ready = '';
select(box, tabs.find((t) => t.getAttribute('href') === location.hash) || tabs[0], false);
}
document.addEventListener('click', (e) => {
const t = e.target.closest('.tabbed[data-ready] > .tabs > a');
if (!t) return;
e.preventDefault();
select(t.closest('.tabbed'), t, false);
});
document.addEventListener('keydown', (e) => {
const t = e.target.closest && e.target.closest('.tabbed[data-ready] > .tabs > a');
if (!t || !['ArrowRight', 'ArrowLeft', 'Home', 'End'].includes(e.key)) return;
e.preventDefault();
const all = [...t.parentElement.children];
const i = all.indexOf(t);
const step = e.key === 'ArrowRight' ? 1 : -1;
const next = e.key === 'Home' ? all[0] : e.key === 'End' ? all[all.length - 1] : all[(i + step + all.length) % all.length];
select(t.closest('.tabbed'), next, true);
});
})();
(() => {
document.addEventListener('click', (e) => {
const a = e.target.closest('.app-side a');
const cb = a && a.closest('.app').querySelector('.app-cb');
if (cb) cb.checked = false;
});
document.addEventListener('keydown', (e) => {
if (e.key !== 'Escape') return;
for (const cb of document.querySelectorAll('.app-cb:checked')) {
cb.checked = false;
cb.focus();
}
});
})();
(() => {
document.addEventListener('click', (e) => {
const link = e.target.closest('[data-embed]');
if (!link || e.metaKey || e.ctrlKey || e.shiftKey || e.button !== 0) return;
const box = link.closest('.video, .media');
if (!box) return;
e.preventDefault();
const f = document.createElement('iframe');
f.src = link.dataset.embed;
f.title = link.dataset.title || link.getAttribute('aria-label') || link.textContent.trim() || 'Video';
f.allow = 'autoplay; encrypted-media; picture-in-picture; fullscreen';
f.allowFullscreen = true;
f.referrerPolicy = 'strict-origin-when-cross-origin';
if (box.classList.contains('video')) {
box.querySelector('.video-load').replaceWith(f);
} else {
const frame = document.createElement('div');
frame.className = 'media-thumb';
frame.append(f);
const thumb = box.querySelector('.media-thumb');
if (thumb) thumb.replaceWith(frame);
else box.prepend(frame);
}
f.focus();
});
})();
(() => {
const ITEMS = '.card, .media, .quote, .gallery-item, li, tr';
const fold = (s) => s.normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase();
const run = (box) => {
const target = box.nextElementSibling;
if (!target) return;
const q = fold(box.querySelector('input').value.trim());
const items = [...target.querySelectorAll(ITEMS)].filter((el) => {
const up = el.parentElement.closest(ITEMS);
return !up || !target.contains(up);
});
let shown = 0;
for (const it of items) {
const hit = !q || fold(it.textContent).includes(q);
it.hidden = !hit;
if (hit) shown++;
}
box.querySelector('.finder-count').textContent = q ? (shown ? shown + ' / ' + items.length : box.dataset.none) : '';
};
for (const box of document.querySelectorAll('.finder')) {
box.hidden = false;
const target = box.nextElementSibling;
if (target && 'MutationObserver' in window) new MutationObserver(() => run(box)).observe(target, { childList: true, subtree: true });
}
document.addEventListener('input', (e) => {
const box = e.target.closest && e.target.closest('.finder');
if (box) run(box);
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
