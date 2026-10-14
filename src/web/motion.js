/*! motion: core */
// Nyra motion (`use web`): the triggers behind the motion words. The CSS does the animating; this
// file only decides when. One IntersectionObserver reveals `data-anim` elements once (with their
// `data-at` children and the children of `stagger` and `draw` containers); one requestAnimationFrame
// loop runs counters, tilt, magnetic pull and the scroll fallback, asleep when nothing needs it and
// stopped in hidden tabs. The build includes each part (marked by a `motion:` comment) only when a page uses it.
(() => {
const D = document, R = D.documentElement,
  RM = matchMedia('(prefers-reduced-motion: reduce)').matches || R.dataset.motion == 'off',
  $ = (s, r = D) => [...r.querySelectorAll(s)],
  setup = [], on = [], subs = new Set();
let raf, last;
R.classList.add('m-on');
// the frame loop: each f(dt, now) runs every frame while it returns true
const frame = (t) => {
  for (const f of subs) f(Math.min(t - last, 50), t) || subs.delete(f);
  last = t;
  raf = subs.size && !D.hidden && requestAnimationFrame(frame);
};
const wake = () => raf || D.hidden || (last = performance.now(), raf = requestAnimationFrame(frame));
const loop = (f) => { subs.add(f); wake(); };
D.addEventListener('visibilitychange', wake);
// reveal once; a container's `data-at` children (and the children of `stagger` and `draw`) with it
const show = (el) => {
  if (el.classList.contains('m-in')) return;
  el.classList.add('m-in');
  for (const f of on) f(el);
  const box = el.matches('[data-anim~=stagger],[data-anim~=draw]');
  for (const c of $('[data-anim]', el))
    c.parentElement.closest('[data-anim]') == el && (box || c.hasAttribute('data-at')) && show(c);
};
const io = new IntersectionObserver((es) => {
  for (const e of es) if (e.isIntersecting) { io.unobserve(e.target); show(e.target); }
}, { rootMargin: '0px 0px -8% 0px' });
const init = () => {
  for (const c of $('[data-anim~=stagger],[data-anim~=draw]'))
    [...c.children].forEach((k, i) => k.style.setProperty('--m-i', i));
  for (const f of setup) f();
  for (const el of $('[data-anim]')) {
    if (RM || el.matches('[data-anim~=none]')) show(el);
    else if (!el.hasAttribute('data-at')) io.observe(el);
    // a sequence step waits for its parent's trigger, or for the page load when it has none
    else if (!el.parentElement.closest('[data-anim]')) requestAnimationFrame(() => show(el));
  }
};
/*! motion: words */
// `words` and `letters`: each word (or letter) of the text in its own span, `--m-j` its index.
setup.push(() => {
  if (RM) return;
  for (const el of $('[data-anim~=words],[data-anim~=letters]')) {
    const L = el.matches('[data-anim~=letters]');
    let j = 0;
    if (L) el.setAttribute('aria-label', el.textContent.trim());
    const span = (c, t) => {
      const s = D.createElement('span');
      s.className = c;
      if (t) { s.textContent = t; s.style.setProperty('--m-j', j++); }
      return s;
    };
    const walk = (n) => {
      for (const c of [...n.childNodes]) {
        if (c.nodeType == 1) { walk(c); continue; }
        if (c.nodeType != 3) continue;
        const f = D.createDocumentFragment();
        for (const p of c.data.split(/([ \t\n\r]+)/)) {
          if (!p || /^[ \t\n\r]/.test(p)) { f.append(p); continue; }
          const w = span(L ? 'm-lw' : 'm-w');
          if (L) { w.ariaHidden = 'true'; for (const ch of p) w.append(span('m-li', ch)); }
          else w.append(span('m-wi', p));
          f.append(w);
        }
        c.replaceWith(f);
      }
    };
    walk(el);
  }
});
/*! motion: count */
// `count`: the first number in the text counts up from 0 ("33+" counts 0...33, then "+"), keeping
// its decimals and thousands separator; it starts with the element's entrance delay.
on.push((el) => {
  if (RM || !el.matches('[data-anim~=count]')) return;
  const w = D.createTreeWalker(el, 4);
  let n, m;
  while ((n = w.nextNode()) && !(m = /(\d{1,3}(?:([   ,.])\d{3})+(?!\d)|\d+)([.,]\d+)?/.exec(n.data)));
  if (!m) return;
  const [all, int, sep, dec = ''] = m, pre = n.data.slice(0, m.index), post = n.data.slice(m.index + all.length),
    end = +(int.replace(/\D/g, '') + '.' + (dec.slice(1) || 0)),
    t0 = performance.now() + (1000 * parseFloat(getComputedStyle(el).animationDelay) || 0),
    fmt = (v) => {
      const [a, b] = v.toFixed(dec.length && dec.length - 1).split('.');
      return pre + (sep ? a.replace(/\B(?=(\d{3})+$)/g, sep) : a) + (b ? dec[0] + b : '') + post;
    };
  loop((dt, t) => {
    const p = Math.min(Math.max((t - t0) / 1600, 0), 1);
    n.data = fmt(p < 1 ? end * (1 - Math.pow(2, -10 * p)) : end);
    return p < 1;
  });
});
/*! motion: draw */
// `draw` on an SVG: each shape gets pathLength 1, so the CSS can draw its stroke with `--draw`.
setup.push(() => {
  for (const s of $('[data-anim~=draw] :is(path,line,polyline,polygon,circle,ellipse,rect)')) s.setAttribute('pathLength', 1);
});
/*! motion: tilt */
// `tilt`: the element turns toward the pointer in 3D with a soft glare, damped every frame.
setup.push(() => {
  if (RM || !matchMedia('(hover: hover) and (pointer: fine)').matches) return;
  const damp = (c, g, dt, ms) => g + (c - g) * Math.exp(-dt / ms);
  for (const el of $('[data-hover~=tilt]')) {
    const g = D.createElement('i'), K = 10 * (parseFloat(getComputedStyle(el).getPropertyValue('--mh-k')) || 1);
    let x = 0, y = 0, h = 0, tx = 0, ty = 0, r = null;
    g.className = 'm-glare';
    g.ariaHidden = 'true';
    el.append(g);
    const tick = (dt) => {
      x = damp(x, tx, dt, 120); y = damp(y, ty, dt, 120); h = damp(h, r ? 1 : 0, dt, 160);
      el.style.transform = `perspective(900px) rotateX(${x.toFixed(2)}deg) rotateY(${y.toFixed(2)}deg) scale(${(1 + h * .02).toFixed(4)})`;
      if (r || Math.abs(x) + Math.abs(y) + h > .01) return true;
      el.style.transform = '';
    };
    el.addEventListener('pointerenter', () => { r = el.getBoundingClientRect(); el.classList.add('m-hot'); loop(tick); });
    el.addEventListener('pointermove', (e) => {
      if (!r) return;
      const px = (e.clientX - r.left) / r.width, py = (e.clientY - r.top) / r.height;
      ty = (px - .5) * K; tx = (.5 - py) * K;
      el.style.setProperty('--mx', px * 100 + '%'); el.style.setProperty('--my', py * 100 + '%');
    });
    el.addEventListener('pointerleave', () => { tx = ty = 0; r = null; el.classList.remove('m-hot'); });
  }
});
/*! motion: magnetic */
// `magnetic`: the element leans toward the pointer and springs back when it leaves.
setup.push(() => {
  if (RM || !matchMedia('(hover: hover) and (pointer: fine)').matches) return;
  for (const el of $('[data-hover~=magnetic]')) {
    const K = parseFloat(getComputedStyle(el).getPropertyValue('--mh-k')) || 1;
    let x = 0, y = 0, vx = 0, vy = 0, tx = 0, ty = 0, r = null;
    const tick = (dt) => {
      // a spring (stiffness 170, damping 14): it follows quickly and overshoots a little on release
      for (let s = dt / 1000; s > 0; s -= .008) {
        const h = Math.min(s, .008);
        vx += (170 * (tx - x) - 14 * vx) * h; vy += (170 * (ty - y) - 14 * vy) * h;
        x += vx * h; y += vy * h;
      }
      el.style.translate = `${x.toFixed(2)}px ${y.toFixed(2)}px`;
      if (r || Math.abs(x) + Math.abs(y) + Math.abs(vx) + Math.abs(vy) > .05) return true;
      el.style.translate = '';
    };
    el.addEventListener('pointerenter', () => { r = el.getBoundingClientRect(); loop(tick); });
    el.addEventListener('pointermove', (e) => {
      if (!r) return;
      tx = (e.clientX - r.left - r.width / 2) * .3 * K; ty = (e.clientY - r.top - r.height / 2) * .4 * K;
    });
    el.addEventListener('pointerleave', () => { tx = ty = 0; r = null; });
  }
});
/*! motion: marquee */
// `marquee`: the children become one group, copied until two groups fill the strip twice over;
// the copies are hidden from screen readers. The speed stays the same for any length.
setup.push(() => {
  for (const m of $('[data-loop~=marquee]')) {
    if (RM || !m.children.length) continue;
    const g = D.createElement('div');
    g.className = 'm-mq';
    g.append(...m.childNodes);
    m.append(g);
    const w = g.offsetWidth || 1;
    for (let i = 1; i < Math.max(2, 2 * Math.ceil(m.offsetWidth / w)) && i < 24; i++) {
      const c = g.cloneNode(true);
      c.ariaHidden = 'true';
      c.inert = true;
      m.append(c);
    }
    m.style.setProperty('--m-mq', w / 60 + 's');
  }
});
/*! motion: scroll */
// The scroll words where CSS scroll-driven animations are missing: the same curves as the CSS,
// computed in the frame loop from positions measured on load and resize.
setup.push(() => {
  const els = $('[data-scroll]');
  if (RM || !els.length || CSS.supports('animation-timeline: view()')) return;
  let vh, items;
  const measure = () => {
    vh = innerHeight;
    items = els.map((el) => {
      el.style.transform = el.style.opacity = '';
      const b = el.getBoundingClientRect();
      return { el, w: el.dataset.scroll, top: b.top + scrollY, h: b.height, r: parseFloat(getComputedStyle(el).getPropertyValue('--ms-r')) || .2 };
    });
  };
  // the progress of an element through the viewport: 0 when its top meets the bottom, 1 when its bottom leaves the top
  const cover = (it) => (scrollY + vh - it.top) / (vh + it.h),
    clamp = (v) => Math.min(Math.max(v, 0), 1);
  const tick = () => {
    for (const it of items) {
      const p = cover(it), s = it.el.style;
      if (/\bprogress\b/.test(it.w)) { const H = R.scrollHeight - vh; s.transform = `scaleX(${H > 0 ? clamp(scrollY / H) : 1})`; }
      else if (/\bparallax\b/.test(it.w)) s.transform = `translate3d(0,${((2 * p - 1) * it.r * (vh + it.h) / 2).toFixed(1)}px,0)`;
      else if (/\bfade-out\b/.test(it.w)) { const q = clamp((p - .35) / .5); s.opacity = 1 - q; s.transform = `translate3d(0,${-q * 18}%,0)`; }
      else if (/\bscale\b/.test(it.w)) { const q = clamp(p / .4); s.transform = `scale(${.88 + .12 * (1 - Math.pow(1 - q, 3))})`; }
    }
  };
  measure();
  tick();
  addEventListener('scroll', () => loop(tick), { passive: true });
  addEventListener('resize', () => { measure(); loop(tick); });
});
/*! motion: end */
D.readyState == 'loading' ? D.addEventListener('DOMContentLoaded', init) : init();
})();
