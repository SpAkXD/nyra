/* Nyra benchmarks page. Vanilla JS, no dependencies; numbers come from data.js (window.NYRA_BENCH).
   One requestAnimationFrame loop that sleeps when nothing needs a frame. Bars morph through CSS custom properties
   (clip-path and transform transitions); JS only writes the target values and tweens the printed numbers. */
(() => {
'use strict';
const D = window.NYRA_BENCH;
if (!D) return;
const $ = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => [...r.querySelectorAll(s)];
const RM = matchMedia('(prefers-reduced-motion: reduce)').matches;
const FINE = matchMedia('(hover: hover) and (pointer: fine)').matches;
const clamp = (v, a = 0, b = 1) => v < a ? a : v > b ? b : v;
const safe = (name, fn) => { try { fn(); } catch (e) { console.error('[nyra-bench:' + name + ']', e); } };
const easeOut = (t) => 1 - Math.pow(1 - t, 3);
const easeBack = (t) => { const c = 1.9; return 1 + (c + 1) * Math.pow(t - 1, 3) + c * Math.pow(t - 1, 2); };
const fmtInt = (n) => Math.round(n).toLocaleString('en-US');
const pct = (n, of) => Math.round(n / of * 100);

/* ---------------- the one loop ---------------- */
const Loop = (() => {
  const subs = new Set(); let raf = 0, last = 0;
  const frame = (now) => {
    const dt = Math.min(now - last, 50); last = now;
    for (const f of subs) if (f(dt, now) === false) subs.delete(f);
    raf = (subs.size && !document.hidden) ? requestAnimationFrame(frame) : 0;
  };
  const wake = () => { if (!raf && subs.size && !document.hidden) { last = performance.now(); raf = requestAnimationFrame(frame); } };
  document.addEventListener('visibilitychange', wake);
  return { add(f) { subs.add(f); wake(); }, del(f) { subs.delete(f); } };
})();
const io = new IntersectionObserver((es) => es.forEach(e => e.target._vis && e.target._vis.forEach(f => f(e.isIntersecting))), { rootMargin: '60px 0px' });
const watch = (node, fn) => { if (!node) return; (node._vis = node._vis || []).push(fn); io.unobserve(node); io.observe(node); };
function whileVisible(node, tick, onShow) {
  let on = false;
  watch(node, (v) => { if (v === on) return; on = v; if (v) { onShow && onShow(); Loop.add(tick); } else Loop.del(tick); });
}
/* call fn once, the first time node scrolls well into view */
function onceVisible(node, fn, margin = '0px 0px -12% 0px') {
  if (!node) return;
  const o = new IntersectionObserver((es) => es.forEach(e => { if (e.isIntersecting) { o.disconnect(); fn(); } }), { rootMargin: margin, threshold: 0.05 });
  o.observe(node);
}

/* ---------------- number tweens ---------------- */
const tweens = new Map();
function tweenTick(dt) {
  for (const [el, tw] of tweens) {
    tw.t += dt;
    const k = easeOut(clamp(tw.t / tw.dur));
    el.textContent = tw.fmt(tw.from + (tw.to - tw.from) * k);
    if (tw.t >= tw.dur) { el._v = tw.to; tweens.delete(el); }
  }
  return tweens.size > 0;
}
function tween(el, to, fmt, dur = 650) {
  if (!el) return;
  const from = el._v != null ? el._v : parseFloat(el.textContent.replace(/[^0-9.\-]/g, '')) || 0;
  if (RM || from === to) { el._v = to; el.textContent = fmt(to); tweens.delete(el); return; }
  const cur = tweens.get(el);
  tweens.set(el, { from: cur ? parseFloat(el.textContent.replace(/[^0-9.\-]/g, '')) || from : from, to, fmt, t: 0, dur });
  el._v = to;
  Loop.add(tweenTick);
}
const fixed = (d) => (v) => v.toFixed(d);

/* ---------------- split headings, reveals ---------------- */
function splitWords(root) {
  let i = 0;
  const walk = (node) => {
    [...node.childNodes].forEach(n => {
      if (n.nodeType === 3) {
        const parts = n.textContent.split(/(\s+)/), f = document.createDocumentFragment();
        parts.forEach(p => {
          if (!p) return;
          if (/^\s+$/.test(p)) { f.append(document.createTextNode(' ')); return; }
          const w = document.createElement('span'); w.className = 'w';
          const wi = document.createElement('span'); wi.className = 'wi'; wi.style.setProperty('--wi', i++); wi.textContent = p;
          w.append(wi); f.append(w);
        });
        n.replaceWith(f);
      } else if (n.nodeType === 1) walk(n);
    });
  };
  walk(root);
}
function reveals() {
  $$('.split').forEach(splitWords);
  if (RM) { $$('.rv, .fb-word').forEach(e => e.classList.add('in')); return; }
  const ro = new IntersectionObserver((es) => es.forEach(e => { if (e.isIntersecting) { e.target.classList.add('in'); ro.unobserve(e.target); } }), { rootMargin: '0px 0px -7% 0px', threshold: 0.08 });
  $$('.rv, .fb-word').forEach(e => ro.observe(e));
}

/* ---------------- header (same behaviour as the home page) ---------------- */
function header() {
  const nav = $('#topbar'), prog = $('#prog'), btn = $('.nav-menu'), sheet = $('#msheet'), links = $('#menu');
  const linkEls = $$('a', links), [il, im, ir] = $$('.nav-ind i', links), ind = $('.nav-ind', links);
  const active = $('a.cur', links);
  let shown = null;
  const moveInd = (a) => {
    if (!a) { ind.classList.remove('on'); shown = null; return; }
    const x = a.offsetLeft, w = a.offsetWidth, h = 36;
    if (!shown) { ind.classList.add('snap'); void ind.offsetWidth; }
    il.style.transform = `translateX(${x}px)`;
    im.style.transform = `translateX(${x + h / 2}px) scaleX(${Math.max(0, w - h)})`;
    ir.style.transform = `translateX(${x + w - h}px)`;
    if (!shown) requestAnimationFrame(() => ind.classList.remove('snap'));
    ind.classList.add('on'); shown = a;
  };
  linkEls.forEach(a => a.addEventListener('pointerenter', () => moveInd(a)));
  links.addEventListener('pointerleave', () => moveInd(active));
  const placeActive = () => { if (getComputedStyle(links).display !== 'none') moveInd(active); };
  placeActive();
  addEventListener('load', placeActive);
  let open = false, closeT = 0;
  const setOpen = (v, focusBack) => {
    if (v === open) return; open = v; clearTimeout(closeT);
    btn.setAttribute('aria-expanded', v); btn.querySelector('.sr').textContent = v ? 'Close menu' : 'Menu';
    document.documentElement.classList.toggle('menu-open', v);
    if (v) { sheet.hidden = false; void sheet.offsetWidth; sheet.classList.add('shown'); setTimeout(() => $('a', sheet).focus({ preventScroll: true }), 60); }
    else { sheet.classList.remove('shown'); closeT = setTimeout(() => { sheet.hidden = true; }, RM ? 0 : 420); if (focusBack) btn.focus(); }
  };
  btn.addEventListener('click', () => setOpen(!open, true));
  sheet.addEventListener('click', (e) => { if (e.target.closest('a')) setOpen(false); });
  addEventListener('keydown', (e) => {
    if (!open) return;
    if (e.key === 'Escape') setOpen(false, true);
    if (e.key === 'Tab') {
      const f = [btn, ...$$('a', sheet)], i = f.indexOf(document.activeElement);
      if (e.shiftKey && i <= 0) { e.preventDefault(); f[f.length - 1].focus(); }
      else if (!e.shiftKey && i === f.length - 1) { e.preventDefault(); f[0].focus(); }
    }
  });
  addEventListener('resize', () => { if (open && innerWidth >= 1000) setOpen(false); placeActive(); });
  const papers = new Set();
  const po = new IntersectionObserver((es) => {
    es.forEach(e => e.isIntersecting ? papers.add(e.target) : papers.delete(e.target));
    nav.classList.toggle('on-paper', papers.size > 0);
  }, { rootMargin: `-36px 0px -${Math.max(0, innerHeight - 38)}px 0px` });
  $$('.sheet').forEach(s => po.observe(s));
  let lastP = -1, sc = null, sy = scrollY;
  const tick = () => {
    const max = Math.max(1, document.documentElement.scrollHeight - innerHeight);
    const p = clamp(sy / max);
    if (Math.abs(p - lastP) > 0.0005) { prog.style.transform = `scaleX(${p.toFixed(4)})`; lastP = p; }
    const s = sy > 24;
    if (s !== sc) { sc = s; nav.classList.toggle('scrolled', s); }
    return false;
  };
  addEventListener('scroll', () => { sy = scrollY; Loop.add(tick); }, { passive: true });
  Loop.add(tick);
}

/* magnetic buttons */
function mags() {
  if (!FINE || RM) return;
  $$('.mag').forEach(b => {
    b.addEventListener('pointermove', (e) => { const r = b.getBoundingClientRect(); b.style.transform = `translate(${(e.clientX - r.left - r.width / 2) * .18}px, ${(e.clientY - r.top - r.height / 2) * .3}px)`; });
    b.addEventListener('pointerleave', () => { b.style.transform = ''; });
  });
}

/* ---------------- segmented switches with a spring knob ---------------- */
function makeSeg(root, onPick) {
  const btns = $$('button', root);
  const knob = document.createElement('span'); knob.className = 'knob snap'; knob.setAttribute('aria-hidden', 'true'); root.prepend(knob);
  let cur = Math.max(0, btns.findIndex(b => b.getAttribute('aria-pressed') === 'true'));
  const place = () => { const b = btns[cur]; if (!b || !b.offsetWidth) return; knob.style.width = b.offsetWidth + 'px'; knob.style.transform = `translateX(${b.offsetLeft - 4}px)`; };
  const select = (i) => { cur = i; btns.forEach((b, k) => b.setAttribute('aria-pressed', k === i)); place(); };
  btns.forEach((b, i) => b.addEventListener('click', () => onPick(i, true)));
  root.addEventListener('keydown', (e) => {
    const k = e.key === 'ArrowRight' ? 1 : e.key === 'ArrowLeft' ? -1 : 0;
    if (!k) return; e.preventDefault(); const j = (cur + k + btns.length) % btns.length; onPick(j, true); btns[j].focus();
  });
  select(cur);
  requestAnimationFrame(() => requestAnimationFrame(() => { place(); knob.classList.remove('snap'); }));
  addEventListener('resize', place);
  addEventListener('load', place);
  return { select, place };
}

/* ---------------- model state, shared by every chart ---------------- */
const Model = (() => {
  let m = 0; const subs = [], segs = [];
  const set = (i, user) => { if (i === m && user) return; m = i; segs.forEach(s => s.select(i)); subs.forEach(f => f(i, user)); };
  return {
    get: () => m,
    on(f) { subs.push(f); f(m, false); },
    bindAll() { $$('.bx-seg[aria-label="Model"]').forEach(el => segs.push(makeSeg(el, (i, user) => set(i, user)))); }
  };
})();
const rowsOf = (chart) => $$(`[data-chart="${chart}"] .bx-row`);

/* ---------------- 01 first try ---------------- */
function passChart() {
  const rows = rowsOf('pass');
  const notes = $$('#first-try .bx-nt');
  Model.on((m, user) => {
    const M = D.models[m];
    rows.forEach((r, l) => {
      r.style.setProperty('--v', (M.pass1[l] / D.run.tasks).toFixed(4));
      r.style.setProperty('--lo', M.ci[l][0]); r.style.setProperty('--hi', M.ci[l][1]);
      if (l === 0) r.style.setProperty('--f', (M.fix.pass / D.run.tasks).toFixed(4));
      tween($('em', r), pct(M.pass1[l], D.run.tasks), fixed(0));
      $('small', r).textContent = `${M.pass1[l]}/${D.run.tasks}`;
    });
    notes.forEach(n => {
      const show = +n.dataset.m === m;
      if (show && n.hidden) { n.hidden = false; if (user && !RM) { n.classList.remove('enter'); void n.offsetWidth; n.classList.add('enter'); } }
      else if (!show) n.hidden = true;
    });
  });
}

function hardDots() {
  const host = $('#hardDots'), sr = $('#hardSr'); if (!host) return;
  host.innerHTML = D.langs.map((name, l) => `<div class="bx-hr${l ? '' : ' ny'}"><b>${name}</b><div class="bx-dots">${'<i></i>'.repeat(28)}</div><span>0</span></div>`).join('');
  const rows = $$('.bx-hr', host);
  rows.forEach(r => $$('i', r).forEach((d, k) => d.style.setProperty('--k', k)));
  let started = RM;
  const paint = (m) => {
    const M = D.models[m];
    rows.forEach((r, l) => {
      const n = started ? M.hard[l] : 0;
      $$('i', r).forEach((d, k) => { d.classList.toggle('on', k < n); d.classList.toggle('miss', started && k >= n); });
      tween($('span', r), n, (v) => `${Math.round(v)}/28`);
    });
    sr.textContent = `${M.name} on the 28 hard tasks, first try: ` + D.langs.map((x, l) => `${x} ${M.hard[l]}`).join(', ') + '.';
  };
  Model.on(paint);
  onceVisible(host, () => { started = true; paint(Model.get()); });
}

/* ---------------- 02 repairs ---------------- */
function repairs() {
  $$('#repairs-grid .bx-rp').forEach(panel => {
    const M = D.models[+panel.dataset.m], N = D.run.tasks;
    $('.bx-rprows', panel).innerHTML = D.langs.map((name, l) => {
      const gain = M.within[l] - M.pass1[l];
      return `<div class="bx-row${l ? '' : ' ny'}" style="--i:${l};--v:${(M.pass1[l] / N).toFixed(4)};--w:${(M.within[l] / N).toFixed(4)}">` +
        `<b>${name}</b><div class="bx-trk"><i class="bx-fill"></i><i class="bx-gain"></i></div>` +
        `<span class="bx-v">${pct(M.within[l], N)}%${gain ? `<span class="bx-plus">+${gain}</span>` : ''}<small>${M.attempts[l].toFixed(2)} tries</small></span></div>`;
    }).join('');
    panel.setAttribute('role', 'img');
    panel.setAttribute('aria-label', `${M.name}, passed within 3 repairs: ` + D.langs.map((x, l) => `${x} ${M.within[l]} of ${N} (first try ${M.pass1[l]}), ${M.attempts[l]} attempts per run`).join('; '));
  });
}

/* ---------------- 03 tokens ---------------- */
function tokens() {
  const rows = rowsOf('tokens'), MAX = 900;
  const ratios = $$('#ratioList li');
  Model.on((m) => {
    const M = D.models[m];
    rows.forEach((r, l) => {
      r.style.setProperty('--v', (M.code[l] / MAX).toFixed(4));
      r.style.setProperty('--g', (M.billed[l] / MAX).toFixed(4));
      tween($('em', r), M.code[l], fixed(0));
      $('small', r).textContent = `${M.billed[l]} billed`;
    });
    [M.ratio.python, M.ratio.typescript, M.ratio.rust].forEach((v, k) => {
      ratios[k].querySelector('i').style.setProperty('--r', v);
      tween(ratios[k].querySelector('em'), v, fixed(2));
    });
  });
}

/* ---------------- 04 cost ---------------- */
function cost() {
  const rows = rowsOf('cost'), MAX = 0.07;
  Model.on((m) => {
    const M = D.models[m];
    const per = M.totalCost.map((c, l) => c / M.within[l]);   // everything billed / programs that passed within 3 repairs
    rows.forEach((r, l) => {
      r.style.setProperty('--v', (per[l] / MAX).toFixed(4));
      tween($('em', r), per[l], fixed(4));
    });
    $('small', rows[0]).textContent = `${(per[0] / per[1]).toFixed(1)}× Python`;
  });
}

function caching() {
  const host = $('#cacheRows'), seg = $('#cacheBoard .bx-steps'); if (!host) return;
  const MAX = 5.5, keys = ['now', 'cached', 'both'];
  host.innerHTML = D.caching.map((c) =>
    `<div class="bx-cm"><h4>${c.model}</h4>` +
    `<div class="bx-row ny" style="--i:0"><b>Nyra</b><div class="bx-trk tall"><i class="bx-fill"></i></div><span class="bx-v">$<em>${c.now.toFixed(2)}</em></span></div>` +
    `<div class="bx-row" style="--i:1;--v:${(c.python / MAX).toFixed(4)}"><b>Python</b><div class="bx-trk tall"><i class="bx-fill"></i></div><span class="bx-v">$<em>${c.python.toFixed(2)}</em></span></div>` +
    `<span class="bx-mult"></span></div>`).join('');
  const blocks = $$('.bx-cm', host);
  let step = 0, touched = false;
  const sg = makeSeg(seg, (i) => { touched = true; show(i); });
  function show(s) {
    step = s; sg.select(s);
    blocks.forEach((b, k) => {
      const c = D.caching[k], v = c[keys[s]], row = $('.bx-row.ny', b);
      row.style.setProperty('--v', (v / MAX).toFixed(4));
      tween($('em', row), v, fixed(2), 800);
      $('.bx-mult', b).textContent = `${(v / c.python).toFixed(1)}× Python's $${c.python.toFixed(2)}`;
    });
  }
  show(0);
  if (!RM) onceVisible(host, () => {
    setTimeout(() => { if (!touched) show(1); }, 1300);
    setTimeout(() => { if (!touched) show(2); }, 3100);
  }, '0px 0px -25% 0px');
}

function abCard() {
  const list = $('#abList'), stack = $('#stack'), btn = $('#cacheTgl');
  if (list) {
    list.innerHTML = D.abCard.arms.map((a, k) =>
      `<li class="${/card/.test(a.arm) ? 'card2' : ''}" style="--k:${k};--p:${(a.pass / a.of).toFixed(4)}"><span>${a.arm}<small>${fmtInt(a.inputPerAttempt)} input tokens per attempt · $${a.cost.toFixed(3)}</small></span><b>${a.pass}/${a.of}</b><i></i></li>`).join('');
  }
  if (!stack) return;
  const MAXC = Math.max(...D.abCard.cache.map(c => c.uncached));
  stack.innerHTML = D.abCard.cache.map(c =>
    `<div class="bx-sk"><div class="bx-sk-h"><span>${c.arm}</span><b>$<em>${c.uncached.toFixed(3)}</em><small></small></b></div>` +
    `<div class="bx-sk-bar"><i class="in-c"></i><i class="out-c"></i></div></div>`).join('') +
    `<p class="bx-sk-k"><i style="background:#F59E0B"></i>input (the spec) <i style="background:var(--teal)"></i>output (program and thinking)</p>`;
  const items = $$('.bx-sk', stack);
  let on = false;
  function set(v) {
    on = v; btn.setAttribute('aria-pressed', v); btn.textContent = v ? 'Cache on' : 'Cache off';
    items.forEach((el, k) => {
      const c = D.abCard.cache[k], inp = v ? c.inputCached : c.inputUncached, tot = v ? c.charged : c.uncached;
      const [a, b] = $$('.bx-sk-bar i', el);
      a.style.transform = `scaleX(${(inp / MAXC).toFixed(4)})`;
      b.style.transform = `translateX(${(inp / MAXC * 100).toFixed(2)}%) scaleX(${(c.output / MAXC).toFixed(4)})`;
      tween($('em', el), tot, fixed(3), 900);
      $('small', el).textContent = v ? `−${c.saved}%` : '';
    });
  }
  set(false);
  btn.addEventListener('click', () => set(!on));
  if (RM) set(true);
  else onceVisible(stack, () => setTimeout(() => set(true), 900), '0px 0px -20% 0px');
}

/* ---------------- 05 speed ---------------- */
function race() {
  const root = $('#race'), lanesEl = $('#lanes'), note = $('#raceNote'), segEl = $('#race .bx-progs'); if (!root) return;
  const P = D.perf, progs = Object.keys(P.programs);
  segEl.setAttribute('aria-label', 'Program');
  segEl.innerHTML = progs.map((p, i) => `<button type="button" aria-pressed="${i === 0}">${p}${p === 'fib' ? '*' : ''}</button>`).join('');
  lanesEl.innerHTML = P.langs.map((l, k) => `<div class="bx-lane${k ? '' : ' ny'}"><b>${l}</b><div class="bx-road"><i class="bx-trail"></i><i class="bx-car"></i></div><span class="bx-time">0 ms<small></small></span></div>`).join('');
  const lanes = $$('.bx-lane', lanesEl).map(el => ({ el, trail: $('.bx-trail', el), car: $('.bx-car', el), time: $('.bx-time', el), done: false }));
  const fmt = (s) => s < 1 ? `${Math.round(s * 1000)} ms` : `${s.toFixed(2)} s`;
  const ordinal = (n) => n + (['th', 'st', 'nd', 'rd'][(n % 100 > 10 && n % 100 < 14) ? 0 : (n % 10 < 4 ? n % 10 : 0)] || 'th');
  let prog = progs[0], t = 0, scale = 1, running = false, rank = [];
  const DUR = 5200;
  const sr = document.createElement('p'); sr.className = 'sr'; sr.setAttribute('aria-live', 'polite'); root.append(sr);
  function setNote() {
    const r = P.ratios[prog], s = P.programs[prog];
    note.textContent = `${prog}: Nyra takes ${r[0].toFixed(2)}× C's time, ${r[1].toFixed(2)}× Rust's, ${r[2].toFixed(2)}× Node's. Python: ${fmt(s[4])}.` + (prog === 'fib' ? ' *gcc folds this recursion.' : '');
    sr.textContent = `${prog}: ` + P.langs.map((l, k) => `${l} ${fmt(s[k])}`).join(', ') + '.';
  }
  function draw(p, lane, secs) {
    lane.trail.style.transform = `scaleX(${p.toFixed(4)})`;
    lane.car.style.transform = `translateX(${(p * 100).toFixed(2)}%)`;
    lane.time.firstChild.textContent = fmt(secs);
  }
  function finish(lane, k) {
    lane.done = true;
    const s = P.programs[prog][k], place = rank[k];
    draw(1, lane, s);
    lane.el.classList.add('done'); lane.el.classList.toggle('p1', place === 1);
    $('small', lane.time).textContent = ordinal(place);
    if (!RM) { lane.el.classList.remove('flash'); void lane.el.offsetWidth; lane.el.classList.add('flash'); }
  }
  function tick(dt) {
    t += dt;
    const s = P.programs[prog];
    let busy = false;
    lanes.forEach((lane, k) => {
      if (lane.done) return;
      const dur = s[k] * scale, p = clamp(t / dur);
      if (p >= 1) finish(lane, k);
      else { draw(p, lane, t / scale); busy = true; }
    });
    if (!busy) running = false;
    return busy;
  }
  function start(p) {
    prog = p; t = 0; setNote();
    const s = P.programs[prog];
    rank = s.map(v => 1 + s.filter(w => w < v).length);   // place by measured time (ties share a place)
    scale = DUR / Math.max(...s);
    lanes.forEach(l => { l.done = false; l.el.classList.remove('done', 'p1', 'flash'); $('small', l.time).textContent = ''; draw(0, l, 0); });
    if (RM) {
      const order = s.map((v, k) => [v, k]).sort((a, b) => a[0] - b[0]);
      order.forEach(([, k]) => finish(lanes[k], k));
      return;
    }
    if (!running) { running = true; Loop.add(tick); }
  }
  const sg = makeSeg(segEl, (i) => { sg.select(i); start(progs[i]); });
  $('#replay').addEventListener('click', () => start(prog));
  setNote();
  lanes.forEach(l => draw(0, l, 0));
  if (RM) start(prog);
  else onceVisible(lanesEl, () => start(prog), '0px 0px -20% 0px');
}

function runtimes() {
  const rows = rowsOf('runtime'), MAX = 600;
  Model.on((m) => {
    const M = D.models[m];
    rows.forEach((r, l) => {
      r.style.setProperty('--v', Math.max(M.runtime[l] / MAX, 0.006).toFixed(4));
      const v = M.runtime[l];
      tween($('em', r), v, v < 10 ? fixed(2) : v < 100 ? fixed(1) : fixed(0));
    });
  });
}

function firstOutput() {
  const host = $('#foRows'); if (!host) return;
  const MAX = 1200, keys = ['cold', 'warm', 'python', 'node'];
  host.innerHTML = D.firstOutput.map((r, n) =>
    `<div class="bx-for"><p><b>${r.prog}</b><span>${r.cold} · ${r.warm} · ${r.python} · ${r.node}</span></p>` +
    keys.map((k, j) => `<i class="c${j + 1}" style="--v:${(r[k] / MAX).toFixed(4)};--k:${n * 4 + j}"></i>`).join('') + '</div>').join('');
  host.setAttribute('role', 'img');
  host.setAttribute('aria-label', 'Time to first output in milliseconds, nyra run cold, cached, Python, Node.js: ' + D.firstOutput.map(r => `${r.prog} ${r.cold}, ${r.warm}, ${r.python}, ${r.node}`).join('; '));
}

/* ---------------- 06 safety ---------------- */
function safetyDemo() {
  const root = $('#safeDemo'); if (!root) return;
  const [pyWin, nyWin] = $$('.bx-t', root);
  const pyRows = $$('.row', pyWin), nyRows = $$('.row', nyWin);
  const pyB = $('#pyBadge'), nyB = $('#nyBadge');
  const badge = (b, txt, cls) => { b.textContent = txt; b.className = 'badge ' + (cls || ''); };
  const final = () => { [...pyRows, ...nyRows].forEach(r => r.classList.add('on')); badge(pyB, 'leaked', 'err'); badge(nyB, 'refused', 'ok'); pyWin.classList.add('hot'); nyWin.classList.add('hot'); };
  if (RM) { final(); return; }
  const reset = () => { [...pyRows, ...nyRows].forEach(r => r.classList.remove('on')); badge(pyB, 'ready'); badge(nyB, 'ready'); pyWin.classList.remove('hot'); nyWin.classList.remove('hot'); };
  const steps = [
    [300, () => { pyRows[0].classList.add('on'); nyRows[0].classList.add('on'); }],
    [700, () => { pyRows[1].classList.add('on'); nyRows[1].classList.add('on'); }],
    [1900, () => { pyRows[2].classList.add('on'); nyRows[2].classList.add('on'); badge(pyB, 'running'); badge(nyB, 'checking'); }],
    [2700, () => { pyRows[3].classList.add('on'); badge(pyB, 'leaked', 'err'); pyWin.classList.add('hot'); }],
    [3000, () => { nyRows[3].classList.add('on'); badge(nyB, 'E0290', 'err'); }],
    [3700, () => { nyRows[4].classList.add('on'); badge(nyB, 'refused', 'ok'); nyWin.classList.add('hot'); }]
  ];
  const END = 10500;
  let t = 0, i = 0;
  reset();
  whileVisible(root, (dt) => {
    t += dt;
    while (i < steps.length && steps[i][0] <= t) steps[i++][1]();
    if (t >= END) { t = 0; i = 0; reset(); }
    return true;
  });
}

function safetyGrid() {
  const host = $('#safeRows'); if (!host) return;
  host.innerHTML = D.safety.map((s, k) =>
    `<li style="--k:${k}"><span class="tt">${s.title}<small>${s.id}<span class="nd2"> · needs ${s.needs}</span></small></span>` +
    `<span class="nd">${s.needs}</span>` +
    `<span class="bx-v2 leak">leaked<small>${s.marker}</small></span>` +
    `<span class="bx-v2 stop">refused<small>E0290, before it ran</small></span></li>`).join('');
}

/* ---------------- hero: the run matrix (996 dots) ---------------- */
function matrix() {
  const fig = $('#matrix'); if (!fig) return;
  const cells = $$('.bx-cell', fig).map(el => {
    const m = +el.dataset.m, l = +el.dataset.l, cv = $('canvas', el);
    const pass = [];
    D.categories.forEach(([name, n]) => {
      const ok = D.categoryPass[m][l][name] != null ? D.categoryPass[m][l][name] : n;
      for (let i = 0; i < n; i++) pass.push(i < ok);
    });
    return { el, m, l, cv, ctx: cv.getContext('2d'), pass, ny: l === 0, w: 0, h: 0 };
  });
  if (!cells[0].ctx) return;
  const COLS = 10, ROWS = 9;
  const DPR = Math.min(devicePixelRatio || 1, 2);
  function size() {
    cells.forEach(c => {
      const w = c.cv.clientWidth, h = c.cv.clientHeight; if (!w || !h) return;
      c.w = w; c.h = h; c.cv.width = Math.round(w * DPR); c.cv.height = Math.round(h * DPR);
      c.ctx.setTransform(DPR, 0, 0, DPR, 0, 0);
    });
  }
  let t = 0, clock = 0;
  const T_CELL = 70, T_DOT = 13, T_POP = 420;
  function draw(final) {
    cells.forEach((c, ci) => {
      const { ctx, w, h } = c; if (!w) return;
      ctx.clearRect(0, 0, w, h);
      const px = w / COLS, py = h / ROWS, r = Math.min(px, py) * 0.36;
      c.pass.forEach((ok, i) => {
        const a = final ? 1 : clamp((t - ci * T_CELL - i * T_DOT) / T_POP);
        if (a <= 0) return;
        const x = (i % COLS + .5) * px, y = (Math.floor(i / COLS) + .5) * py;
        let rr = r * (final ? 1 : easeBack(a));
        if (ok) ctx.fillStyle = c.ny ? '#0BD8B6' : 'rgba(203,213,225,.5)';
        else {
          const pulse = .6 + .4 * Math.sin(clock * 3.2 + i * .7);
          ctx.fillStyle = `rgba(248,113,113,${final && RM ? 1 : (.55 + .45 * pulse).toFixed(3)})`;
          rr *= 1 + (final && RM ? 0 : .12 * pulse);
          if (!RM) { ctx.globalAlpha = .25 * pulse; ctx.beginPath(); ctx.arc(x, y, rr * 2.1, 0, 6.2832); ctx.fill(); ctx.globalAlpha = 1; }
        }
        ctx.beginPath(); ctx.arc(x, y, Math.max(0, rr), 0, 6.2832); ctx.fill();
      });
    });
  }
  size();
  if ('ResizeObserver' in window) new ResizeObserver(() => { size(); draw(RM || t > 4000); }).observe(fig);
  if (RM) { draw(true); return; }
  const END = cells.length * T_CELL + 83 * T_DOT + T_POP;
  whileVisible(fig, (dt) => {
    t += dt; clock += dt / 1000;
    draw(false);
    return true;
  });
  void END;
}

/* hero count-ups */
function counts() {
  const els = $$('[data-count]'); if (RM) return;
  els.forEach(el => {
    const to = parseFloat(el.dataset.count), dec = +(el.dataset.dec || 0);
    el._v = 0; el.textContent = (0).toFixed(dec);
    onceVisible(el, () => setTimeout(() => tween(el, to, fixed(dec), 1500), 250), '0px 0px -5% 0px');
  });
}

/* ---------------- boot ---------------- */
safe('reveals', reveals);
safe('header', header);
safe('mags', mags);
safe('repairs', repairs);
safe('hard', hardDots);
safe('pass', passChart);
safe('tokens', tokens);
safe('cost', cost);
safe('runtimes', runtimes);
safe('models', () => Model.bindAll());
safe('caching', caching);
safe('ab', abCard);
safe('race', race);
safe('firstOutput', firstOutput);
safe('safetyDemo', safetyDemo);
safe('safetyGrid', safetyGrid);
safe('matrix', matrix);
safe('counts', counts);
})();
