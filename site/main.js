/* Nyra site. Vanilla JS, no dependencies. One shared rAF loop that only runs while a demo is on screen. */
(() => {
'use strict';
const $ = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => [...r.querySelectorAll(s)];
const RM = matchMedia('(prefers-reduced-motion: reduce)').matches;
const el = (t, c, txt) => { const e = document.createElement(t); if (c) e.className = c; if (txt != null) e.textContent = txt; return e; };
const safe = (fn) => { try { fn(); } catch (e) { console.error(e); } };

/* ---------- shared ticker: one rAF loop, runs only while something is subscribed and the tab is visible ---------- */
const Ticker = (() => {
  const subs = new Set(); let raf = 0, last = 0;
  const frame = (now) => { raf = requestAnimationFrame(frame); const dt = Math.min(now - last, 60); last = now; subs.forEach(f => f(dt)); };
  const start = () => { if (!raf && subs.size && !document.hidden) { last = performance.now(); raf = requestAnimationFrame(frame); } };
  const stop = () => { if (raf) { cancelAnimationFrame(raf); raf = 0; } };
  document.addEventListener('visibilitychange', () => document.hidden ? stop() : start());
  return { add(f) { subs.add(f); start(); }, del(f) { subs.delete(f); if (!subs.size) stop(); } };
})();

/* run `tick` only while `node` is on screen */
const io = ('IntersectionObserver' in window) ? new IntersectionObserver((es) => es.forEach(e => e.target._vis && e.target._vis(e.isIntersecting)), { rootMargin: '60px 0px' }) : null;
function whenVisible(node, onChange) { node._vis = onChange; if (io) io.observe(node); else onChange(true); }
function animate(node, tick, onStart) {
  let on = false;
  whenVisible(node, (v) => { if (v === on) return; on = v; if (v) { onStart && onStart(); Ticker.add(tick); } else Ticker.del(tick); });
}

/* a timeline: [[ms, fn], ...] driven by dt */
function timeline(steps, dur, onEnd) {
  steps.sort((a, b) => a[0] - b[0]);
  let t = 0, i = 0;
  return {
    reset() { t = 0; i = 0; },
    tick(dt) { t += dt; while (i < steps.length && steps[i][0] <= t) steps[i++][1](); if (t >= dur) onEnd(); }
  };
}

/* ---------- tiny syntax highlighter ---------- */
const LANG = {
  nyra: { kw: /^(fn|let|var|if|else|while|for|in|ret|struct|inout|break|continue|arena|true|false)$/, ty: /^(int|float|bool|str|char)$/ },
  ir: { kw: /^(fn|return|call|print|loop|while|step)$/, ty: /^(int|float|bool|str|char)$/ },
  c: { kw: /^(static|return|void|const|if|else|for|while)$/, ty: /^(int64_t|int|char|void|double|bool)$/ },
  js: { kw: /^(function|return|let|const|if|else|for|while)$/, ty: /^$/ }
};
const TOK = /(\/\/[^\n]*)|("(?:[^"\\\n]|\\.)*")|('(?:[^'\\\n]|\\.)')|(%\d+)|(\d+(?:\.\d+)?)|([A-Za-z_]\w*)|(\s+)|([\s\S])/g;
function hl(src, lang) {
  const L = LANG[lang] || LANG.nyra, out = []; let m, prevWord = '';
  TOK.lastIndex = 0;
  while ((m = TOK.exec(src))) {
    if (m[1]) out.push(['c', m[1]]);
    else if (m[2]) {
      if (lang === 'nyra' && m[2].includes('{')) m[2].split(/(\{[^{}]*\})/).forEach(p => p && out.push([/^\{[^{}]+\}$/.test(p) ? 'si' : 's', p]));
      else out.push(['s', m[2]]);
    }
    else if (m[3]) out.push(['s', m[3]]);
    else if (m[4] || m[5]) out.push(['n', m[4] || m[5]]);
    else if (m[6]) {
      const w = m[6], nx = src[TOK.lastIndex];
      let c = '';
      if (L.kw.test(w)) c = 'k';
      else if (nx === '(' && L.ty.test(w)) c = 'b';
      else if (L.ty.test(w)) c = 't';
      else if (nx === '(') c = 'f';
      else if (/^[A-Z]/.test(w)) c = 't';
      out.push([c, w]); prevWord = w; continue;
    }
    else if (m[7]) out.push(['', m[7]]);
    else out.push(['p', m[8]]);
    prevWord = '';
  }
  return out;
}
/* build highlighted lines into `pre`; with perChar returns one entry per source char: {s, row, col} (col = column after the char) */
function build(pre, src, lang, perChar) {
  const frag = document.createDocumentFragment(); let line = el('div', 'l'); frag.append(line);
  const chars = []; let row = 0, col = 0;
  for (const [c, t] of hl(src, lang)) {
    t.split('\n').forEach((part, pi) => {
      if (pi > 0) { line = el('div', 'l'); frag.append(line); row++; col = 0; if (perChar) chars.push({ s: null, row, col: 0 }); }
      if (!part) return;
      if (perChar) for (const ch of part) { const s = el('span', c, ch); line.append(s); chars.push({ s, row, col: ++col }); }
      else { line.append(el('span', c, part)); col += part.length; }
    });
  }
  pre.replaceChildren(frag);
  return chars;
}
const langOf = (p) => p.classList.contains('ir') ? 'ir' : p.classList.contains('lang-c') ? 'c' : p.classList.contains('lang-js') ? 'js' : 'nyra';
function lineFrag(src, lang) { const f = document.createDocumentFragment(); hl(src, lang || 'nyra').forEach(([c, t]) => f.append(el('span', c, t))); return f; }

/* ---------- reveal on scroll ---------- */
function reveal() {
  const items = $$('.rv');
  if (RM || !io) { items.forEach(n => n.classList.add('in')); return; }
  const ro = new IntersectionObserver((es) => es.forEach(e => { if (e.isIntersecting) { e.target.classList.add('in'); ro.unobserve(e.target); } }), { rootMargin: '0px 0px -8% 0px', threshold: .08 });
  items.forEach(n => ro.observe(n));
}

/* ---------- header, nav, scroll progress (one passive scroll handler) ---------- */
const scrollHooks = [];
function chrome() {
  const bar = $('#topbar'), prog = $('#prog'), burger = $('.burger'), menu = $('#menu');
  burger.addEventListener('click', () => { const o = burger.getAttribute('aria-expanded') === 'true'; burger.setAttribute('aria-expanded', String(!o)); menu.classList.toggle('open', !o); });
  menu.addEventListener('click', (e) => { if (e.target.closest('a')) { burger.setAttribute('aria-expanded', 'false'); menu.classList.remove('open'); } });
  let q = false;
  const upd = () => {
    q = false;
    const y = scrollY, h = document.documentElement.scrollHeight - innerHeight;
    bar.classList.toggle('sc', y > 8);
    prog.style.transform = 'scaleX(' + (h > 0 ? Math.min(1, y / h) : 0).toFixed(4) + ')';
    scrollHooks.forEach(f => f());
  };
  addEventListener('scroll', () => { if (!q) { q = true; requestAnimationFrame(upd); } }, { passive: true });
  addEventListener('resize', () => requestAnimationFrame(upd));
  upd();
  if (io) {
    const links = new Map($$('a', menu).map(a => [a.getAttribute('href').slice(1), a]));
    const so = new IntersectionObserver((es) => es.forEach(e => { const a = links.get(e.target.id); if (a) a.classList.toggle('on', e.isIntersecting); }), { rootMargin: '-45% 0px -50% 0px' });
    links.forEach((a, id) => { const s = document.getElementById(id); if (s) so.observe(s); });
  }
}

/* ---------- hero: an agent writes a program, it compiles, native and JS print the same ---------- */
function hero() {
  const root = $('#stage'), data = JSON.parse($('#hero-data').textContent);
  const sfile = $('#sfile'), stask = $('#stask'), sgut = $('#sgut'), scode = $('#scode'), phase = $('#phase'), phaseT = $('#phase-t'), dots = $('#sdots');
  const outs = $$('.out-b', root), cmdFor = (d, js) => '$ nyra run ' + d.file + (js ? ' --js' : '');
  const btns = data.map((d, i) => { const b = el('button', '', d.file); b.type = 'button'; b.addEventListener('click', () => { pick(i); }); dots.append(b); return b; });
  let pi = 0, chars = [], plan = [], tl, caret, t = 0, lh = 20, idx = 0, pl = 0, running = false, shown = 0;
  const setPhase = (txt, cls) => { phaseT.textContent = txt; phase.className = 'sp phase ' + (cls || ''); };
  const mkPlan = (src) => { let tt = 0, i = 0, seed = 11; const r = () => (seed = seed * 16807 % 2147483647) / 2147483647, out = [];
    while (i < src.length) { let end = Math.min(src.length, i + 2 + Math.floor(r() * 4)); const nl = src.indexOf('\n', i); if (nl >= 0 && nl < end) end = nl + 1; i = end; tt += 34 + r() * 44 + (src[end - 1] === '\n' ? 100 + r() * 120 : 0); out.push([end, tt]); } return out; };
  const reveal = (n) => { for (; shown < n; shown++) { const c = chars[shown]; if (c && c.s) c.s.classList.add('on'); } const c = chars[n - 1]; if (c) caret.style.transform = 'translate(' + c.col + 'ch,' + c.row * lh + 'px)'; };
  function load(i) {
    pi = i; const d = data[i];
    sfile.textContent = d.file; stask.textContent = d.task;
    sgut.textContent = Array.from({ length: 10 }, (_, k) => k + 1).join('\n');
    chars = build(scode, d.code, 'nyra', true);
    caret = el('i', 'caret'); scode.append(caret); caret.style.transform = 'translate(0,0)';
    lh = parseFloat(getComputedStyle(scode).lineHeight) || 20;
    shown = 0; plan = mkPlan(d.code); pl = 0;
    outs.forEach((o, k) => { o.classList.remove('on'); o.replaceChildren(); [cmdFor(d, k)].concat(d.out).forEach((txt, n) => { const s = el('span', n ? '' : 'cmd', txt); s.style.setProperty('--i', n + k); o.append(s, document.createTextNode(n < d.out.length ? '\n' : '')); }); });
    root.classList.remove('same'); caret.classList.remove('on');
    btns.forEach((b, k) => b.setAttribute('aria-pressed', k === pi));
    setPhase('thinking', 'live');
  }
  function finalState() {
    reveal(chars.length); caret.classList.remove('on');
    outs.forEach(o => o.classList.add('on')); root.classList.add('same'); setPhase('identical', 'ok');
  }
  function makeTl() {
    const W = 700, end = W + plan[plan.length - 1][1];
    const steps = [
      [0, () => setPhase('thinking', 'live')],
      [W, () => { caret.classList.add('on'); setPhase('writing', 'live'); }],
      [end + 250, () => { caret.classList.remove('on'); setPhase('checking', 'live'); }],
      [end + 1000, () => setPhase('check ok', 'ok')],
      [end + 1350, () => { outs.forEach(o => o.classList.add('on')); setPhase('running', 'live'); }],
      [end + 2300, () => { root.classList.add('same'); setPhase('identical', 'ok'); }]
    ];
    return { W, t: timeline(steps, end + 2300 + 5200, () => start((pi + 1) % data.length)) };
  }
  function start(i) { load(i); tl = makeTl(); t = 0; pl = 0; }
  function pick(i) { if (RM) { load(i); finalState(); } else start(i); }
  load(0);
  if (RM) { finalState(); return; }
  tl = makeTl();
  animate(root, (dt) => {
    tl.t.tick(dt); t += dt;
    while (pl < plan.length && plan[pl][1] + tl.W <= t) { reveal(plan[pl][0]); pl++; }
  });
}

/* ---------- errors: code, position, hint, fix, run ---------- */
function errors() {
  const root = $('#err'), code = $('#ecode'), term = $('#eterm'), badge = $('#ebadge');
  const old = ['fn main() {', '    var xs = []', '    xs.push(1)', '    print(xs)', '}'];
  const squig = $('.squig', code);
  const frag = document.createDocumentFragment();
  old.forEach((ln, i) => {
    const d = el('div', 'l');
    if (i === 1) {
      d.className = 'l swap';
      const a = el('span', 'a'), b = el('span', 'b'), ins = el('span', 'ins');
      a.append(lineFrag(ln)); b.append(lineFrag('    var xs')); ins.append(lineFrag(': [int]')); b.append(ins); b.append(lineFrag(' = []'));
      d.append(a, b);
    } else d.append(lineFrag(ln));
    frag.append(d);
  });
  code.append(frag);
  const rows = $$('.row', term), jh = $$('.jh', term);
  const showRow = (n) => rows[n].classList.remove('h');
  const setB = (txt, cls) => { badge.textContent = txt; badge.className = 'badge ' + cls; };
  const jhOn = (k) => jh.forEach(j => { if (j.dataset.k === k) j.classList.add('hl'); });
  const reset = () => { root.classList.remove('show-err', 'fixed', 'fixing'); rows.forEach(r => r.classList.add('h')); jh.forEach(j => j.classList.remove('hl')); setB('unchecked', ''); };
  const finalState = () => { root.classList.add('fixed'); root.classList.remove('show-err'); rows.forEach(r => r.classList.remove('h')); jh.forEach(j => j.classList.remove('hl')); setB('0 errors', 'ok'); };
  if (RM) { finalState(); return; }
  reset();
  const tl = timeline([
    [700, () => showRow(0)],
    [1300, () => { showRow(1); setB('1 error', 'err'); }],
    [1900, () => jhOn('code')],
    [2500, () => { jhOn('pos'); root.classList.add('show-err'); }],
    [3300, () => jhOn('hint')],
    [5200, () => { setB('applying the hint', 'fix'); root.classList.add('fixing', 'fixed'); root.classList.remove('show-err'); }],
    [6400, () => showRow(2)],
    [6900, () => { showRow(3); setB('0 errors', 'ok'); root.classList.remove('fixing'); }],
    [7700, () => showRow(4)],
    [8200, () => showRow(5)]
  ], 13500, () => { reset(); tl.reset(); });
  animate(root, tl.tick, () => { reset(); tl.reset(); });
}

/* ---------- pipeline ---------- */
function pipeline() {
  const root = $('#pipe'), lis = $$('#steps li'), fill = $('#sfill'), scenes = $$('.scene', root);
  const N = lis.length, dwell = [3000, 3300, 3300, 3300, 4200, 4200, 4200];
  let cur = 0, t = 0;
  const sc = scenes.map(s => ({ n: s, set: s.dataset.s.split(',').map(Number) }));
  function go(i) {
    cur = i; t = 0;
    lis.forEach((li, k) => { li.classList.toggle('cur', k === i); li.classList.toggle('past', k < i); });
    fill.style.transform = 'scaleX(' + (i / (N - 1)) + ')';
    sc.forEach(s => { s.n.classList.toggle('on', s.set.includes(i)); s.n.classList.toggle('t3', i === 3); });
  }
  lis.forEach((li, k) => { const b = $('button', li); b.prepend(el('i', 'nd')); b.addEventListener('click', () => go(k)); });
  const finalAll = RM;
  go(0);
  if (finalAll) return;
  animate(root, (dt) => { t += dt; if (t >= dwell[cur]) go((cur + 1) % N); });
}

/* ---------- copy on write ---------- */
function cow() {
  const root = $('#cow'), g = (id) => document.getElementById(id), code = $('#ccode'), cout = $('#cout');
  const lines = ['fn main() {', '    var a = [1, 2]', '    var b = a', '    b.push(3)', '    a[0] = 9', '    print(a)', '    print(b)', '}'];
  build(code, lines.join('\n'), 'nyra', false);
  const hlb = el('div', 'hlb'); code.append(hlb);
  const lh = () => parseFloat(getComputedStyle(code).lineHeight) || 20;
  const row = (n) => { hlb.style.transform = 'translateY(' + n * lh() + 'px)'; };
  const b1 = g('c-b1'), b2 = g('c-b2'), c3 = g('c-c3'), ta = g('c-ta'), tb = g('c-tb'), pa = g('c-pa'), pb1 = g('c-pb1'), pb2 = g('c-pb2'), rc1 = g('c-rc1'), v0 = g('c-v0'), cap = g('ccap');
  const rcs = $$('.rc text', root), cell0 = v0.parentNode;
  const on = (...n) => n.forEach(x => { x.classList.remove('off'); x.classList.add('on'); });
  const pop = (n) => n.animate && n.animate([{ transform: 'scale(1.45)' }, { transform: 'scale(1)' }], { duration: 380, easing: 'cubic-bezier(.22,1,.36,1)' });
  const say = (t) => { cap.textContent = t; };
  const reset = () => {
    root.classList.remove('run'); hlb.style.transform = 'translateY(0)';
    [b1, b2, c3, ta, tb, pa, pb1, pb2].forEach(n => n.classList.remove('on', 'off'));
    rcs.forEach(r => r.textContent = 'rc 1'); v0.textContent = '1'; cell0.classList.remove('hit'); cout.classList.remove('on');
    say('A value is an array of two numbers.');
  };
  const finalState = () => {
    root.classList.add('run'); row(6); on(b1, b2, c3, ta, tb, pa, pb2); pb1.classList.add('off'); v0.textContent = '9'; cout.classList.add('on');
    say('Two independent values. Neither write leaked into the other.');
  };
  if (RM) { finalState(); return; }
  reset();
  const tl = timeline([
    [400, () => { root.classList.add('run'); row(1); on(b1, ta, pa); say('a owns one array of two numbers.'); }],
    [2300, () => { row(2); on(tb, pb1); rcs[0].textContent = 'rc 2'; pop(rcs[0]); say('var b = a copies nothing. Both names share one array, and its count says two owners.'); }],
    [4800, () => { row(3); say('b wants to write, but the array has two owners. So b gets a private copy first: copy on write.'); on(b2); }],
    [5300, () => { pb1.classList.remove('on'); pb1.classList.add('off'); on(pb2); rcs[0].textContent = 'rc 1'; pop(rcs[0]); }],
    [6100, () => { on(c3); }],
    [8000, () => { row(4); say('a is the only owner now, so it writes in place. No copy at all.'); v0.textContent = '9'; cell0.classList.add('hit'); pop(v0); }],
    [9800, () => { row(5); cell0.classList.remove('hit'); say('Two independent values. Neither write leaked into the other.'); cout.classList.add('on'); }],
    [10300, () => row(6)]
  ], 15000, () => { reset(); tl.reset(); });
  animate(root, tl.tick, () => { reset(); tl.reset(); });
}

/* ---------- tabs, copy buttons ---------- */
function tabs() {
  const root = $('#tour'), btns = $$('[role=tab]', root), panels = $$('[role=tabpanel]', root);
  btns.forEach((b, i) => { b.id = 'tab' + i; panels[i].setAttribute('aria-labelledby', b.id); b.setAttribute('aria-controls', 'tp' + i); panels[i].id = 'tp' + i; });
  const sel = (i, focus) => {
    btns.forEach((b, k) => { b.setAttribute('aria-selected', k === i); b.tabIndex = k === i ? 0 : -1; });
    panels.forEach((p, k) => { p.hidden = k !== i; });
    if (focus) btns[i].focus();
  };
  btns.forEach((b, i) => {
    b.addEventListener('click', () => sel(i));
    b.addEventListener('keydown', (e) => {
      const k = { ArrowRight: 1, ArrowLeft: -1 }[e.key];
      if (k) { e.preventDefault(); sel((i + k + btns.length) % btns.length, true); }
      else if (e.key === 'Home') { e.preventDefault(); sel(0, true); } else if (e.key === 'End') { e.preventDefault(); sel(btns.length - 1, true); }
    });
  });
  sel(0);
}
function copyButtons() {
  $$('pre[data-copy]').forEach(pre => {
    const text = pre.textContent.replace(/\n$/, '');
    const b = el('button', 'cp', 'Copy'); b.type = 'button'; b.setAttribute('aria-label', 'Copy code');
    b.addEventListener('click', async () => {
      let ok = false;
      try { await navigator.clipboard.writeText(text); ok = true; } catch (e) {
        const ta = el('textarea'); ta.value = text; ta.style.cssText = 'position:fixed;opacity:0'; document.body.append(ta); ta.select();
        try { ok = document.execCommand('copy'); } catch (e2) { } ta.remove();
      }
      b.textContent = ok ? 'Copied' : 'Press Ctrl+C'; setTimeout(() => b.textContent = 'Copy', 1600);
    });
    pre.parentNode.append(b);
  });
}

/* ---------- roadmap drawn by scroll ---------- */
function roadmap() {
  const road = $('#road'), rf = $('#rf'), rh = $('#rh'), lis = $$('li', road), rt = $('.rt', road);
  let H = 0, ys = [], vis = false;
  const measure = () => {
    H = rt.offsetHeight; ys = lis.map(li => li.offsetTop + 15 - 10);
    const now = lis.findIndex(li => li.classList.contains('now')); if (now >= 0) road.style.setProperty('--now', (ys[now] / H * 100).toFixed(1) + '%');
  };
  const set = (p) => {
    rf.style.transform = 'scaleY(' + p.toFixed(4) + ')';
    rh.style.transform = 'translateY(' + (p * (H - 4)).toFixed(1) + 'px)'; rh.style.opacity = p > 0 && p < 1 ? 1 : 0;
    lis.forEach((li, i) => li.classList.toggle('on', p * H >= ys[i] - 6));
  };
  measure();
  if (RM) { set(1); return; }
  set(0);
  addEventListener('resize', measure);
  whenVisible(road, (v) => { vis = v; road.classList.toggle('run', v); });
  scrollHooks.push(() => {
    if (!vis) return;
    const r = road.getBoundingClientRect();
    set(Math.max(0, Math.min(1, (innerHeight * .62 - r.top - 10) / H)));
  });
}

/* ---------- small looping CSS animations: paused unless on screen ---------- */
function runWhenVisible() { ['#loop', '#benchbox'].forEach(s => { const n = $(s); if (n) whenVisible(n, v => n.classList.toggle('run', v)); }); }

reveal();
safe(chrome); safe(hero); safe(errors); safe(pipeline); safe(cow); safe(tabs); safe(copyButtons); safe(roadmap); safe(runWhenVisible);
$$('pre.nyra[data-copy], .pcode').forEach(p => safe(() => build(p, p.textContent.replace(/\n$/, ''), langOf(p), false)));
})();
