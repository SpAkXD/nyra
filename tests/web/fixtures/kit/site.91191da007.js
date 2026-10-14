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
document.addEventListener('click', (e) => {
const link = e.target.closest('.map-load');
const frame = link && link.closest('.map-frame');
if (!frame || !frame.dataset.src || e.metaKey || e.ctrlKey) return;
e.preventDefault();
const f = document.createElement('iframe');
f.src = frame.dataset.src;
f.title = frame.dataset.title || '';
f.loading = 'lazy';
f.referrerPolicy = 'no-referrer-when-downgrade';
f.allowFullscreen = true;
frame.replaceChildren(f);
});
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
const body = (message ? message.value.trim() + '\n\n—\n' : '') + lines.join('\n');
const subject = form.dataset.subject ? 'subject=' + encodeURIComponent(form.dataset.subject) + '&' : '';
window.location.href = 'mailto:' + form.dataset.mailto + '?' + subject + 'body=' + encodeURIComponent(body);
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
