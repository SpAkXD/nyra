/* Nyra site. Vanilla JS, no dependencies.
   One requestAnimationFrame loop for everything; it sleeps when nothing on screen needs a frame
   and when the tab is hidden. Scroll handlers only record scrollY; all reads of layout happen in
   measure() (on load and resize), all writes happen inside the loop. */
(() => {
'use strict';
const $ = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => [...r.querySelectorAll(s)];
const RM = matchMedia('(prefers-reduced-motion: reduce)').matches;
const FINE = matchMedia('(hover: hover) and (pointer: fine)').matches;
const clamp = (v, a = 0, b = 1) => v < a ? a : v > b ? b : v;
const damp = (cur, target, dt, ms) => target + (cur - target) * Math.exp(-dt / ms);
const sstep = (a, b, x) => { const t = clamp((x - a) / (b - a)); return t * t * (3 - 2 * t); };
const safe = (name, fn) => { try { fn(); } catch (e) { console.error('[nyra:' + name + ']', e); } };

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

/* visibility via one IntersectionObserver */
const io = new IntersectionObserver((es) => es.forEach(e => e.target._vis && e.target._vis(e.isIntersecting)), { rootMargin: '80px 0px' });
const watch = (node, fn) => { if (!node) return; node._vis = fn; io.observe(node); };
/* run tick(dt, now) every frame while node is on screen */
function whileVisible(node, tick, onShow) {
  let on = false;
  watch(node, (v) => { if (v === on) return; on = v; if (v) { onShow && onShow(); Loop.add(tick); } else Loop.del(tick); });
}

/* ---------------- layout cache and scroll ---------------- */
const L = { vh: innerHeight, vw: innerWidth, sy: scrollY, doc: 1 };
const scenes = [];           // { el, vis, measure(), update(dt) -> busy }
const absTop = (el) => el.getBoundingClientRect().top + scrollY;
function measure() {
  L.vh = innerHeight; L.vw = innerWidth; L.sy = scrollY;
  L.doc = document.documentElement.scrollHeight;
  scenes.forEach(s => s.measure && s.measure());
  Loop.add(scrollTick);
}
function scrollTick(dt) {
  let busy = false;
  for (const s of scenes) if (s.vis && s.update(dt)) busy = true;
  return busy;
}
addEventListener('scroll', () => { L.sy = scrollY; Loop.add(scrollTick); }, { passive: true });
let rsT = 0;
addEventListener('resize', () => { clearTimeout(rsT); rsT = setTimeout(measure, 120); });
function scene(el, s) {
  s.el = el; s.vis = false; scenes.push(s);
  watch(el, (v) => { s.vis = v; if (v) Loop.add(scrollTick); });
  return s;
}

/* ---------------- syntax highlighting ---------------- */
const LANG = {
  nyra: { kw: /^(fn|let|var|if|else|while|for|in|ret|struct|inout|break|continue|arena|true|false)$/, ty: /^(int|float|bool|str|char)$/ },
  ir: { kw: /^(fn|return|call|print)$/, ty: /^(int|float|bool|str|char)$/ },
  c: { kw: /^(static|return|void|const|if|else|for|while)$/, ty: /^(int64_t|int|char|void|double|bool)$/ },
  js: { kw: /^(function|return|let|const|if|else|for|while)$/, ty: /^$/ }
};
const TOK = /(\/\/[^\n]*)|("(?:[^"\\\n]|\\.)*")|('(?:[^'\\\n]|\\.)')|(%\d+)|(\d+(?:\.\d+)?)|([A-Za-z_]\w*)|(\s+)|([\s\S])/g;
const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
function hlInline(src, lang = 'nyra') {
  const Lg = LANG[lang] || LANG.nyra; let m, out = '';
  const sp = (c, t) => c ? `<span class="${c}">${esc(t)}</span>` : esc(t);
  TOK.lastIndex = 0;
  while ((m = TOK.exec(src))) {
    if (m[1]) out += sp('c', m[1]);
    else if (m[2]) {
      if (lang === 'nyra' && m[2].includes('{')) m[2].split(/(\{[^{}]*\})/).forEach(p => { if (p) out += sp(/^\{[^{}]+\}$/.test(p) ? 'si' : 's', p); });
      else out += sp('s', m[2]);
    }
    else if (m[3]) out += sp('s', m[3]);
    else if (m[4] || m[5]) out += sp('n', m[4] || m[5]);
    else if (m[6]) {
      const w = m[6], nx = src[TOK.lastIndex]; let c = '';
      if (Lg.kw.test(w)) c = 'k';
      else if (nx === '(' && Lg.ty.test(w)) c = 'b';
      else if (Lg.ty.test(w)) c = 't';
      else if (nx === '(') c = 'f';
      else if (/^[A-Z]/.test(w)) c = 't';
      out += sp(c, w);
    }
    else if (m[7]) out += m[7];
    else out += sp('p', m[8]);
  }
  return out;
}
const hlLines = (src, lang) => src.split('\n').map(l => `<div class="l">${hlInline(l, lang) || ' '}</div>`).join('');

/* ---------------- split headings into words ---------------- */
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

/* ---------------- reveals ---------------- */
function reveals() {
  $$('.split').forEach(splitWords);
  if (RM) { $$('.rv').forEach(e => e.classList.add('in')); return; }
  const ro = new IntersectionObserver((es) => es.forEach(e => { if (e.isIntersecting) { e.target.classList.add('in'); ro.unobserve(e.target); } }), { rootMargin: '0px 0px -7% 0px', threshold: 0.08 });
  $$('.rv').forEach(e => ro.observe(e));
}

/* ---------------- header ---------------- */
function header() {
  const top = $('#topbar'), prog = $('#prog'), burger = $('.burger'), menu = $('#menu');
  burger.addEventListener('click', () => {
    const open = burger.getAttribute('aria-expanded') !== 'true';
    burger.setAttribute('aria-expanded', open); menu.classList.toggle('open', open);
  });
  menu.addEventListener('click', (e) => { if (e.target.closest('a')) { burger.setAttribute('aria-expanded', 'false'); menu.classList.remove('open'); } });
  addEventListener('keydown', (e) => { if (e.key === 'Escape' && menu.classList.contains('open')) { burger.click(); burger.focus(); } });
  /* header turns cream over paper sheets */
  const papers = new Set();
  const po = new IntersectionObserver((es) => {
    es.forEach(e => e.isIntersecting ? papers.add(e.target) : papers.delete(e.target));
    top.classList.toggle('on-paper', papers.size > 0);
  }, { rootMargin: `-${32}px 0px -${Math.max(0, innerHeight - 34)}px 0px` });
  $$('.sheet').forEach(s => po.observe(s));
  /* current section in nav */
  const links = new Map($$('a', menu).map(a => [a.getAttribute('href').slice(1), a]));
  const so = new IntersectionObserver((es) => es.forEach(e => {
    const a = links.get(e.target.id); if (!a) return;
    if (e.isIntersecting) { links.forEach(x => x.classList.remove('cur')); a.classList.add('cur'); }
  }), { rootMargin: '-45% 0px -50% 0px' });
  links.forEach((a, id) => { const s = document.getElementById(id); if (s) so.observe(s); });
  /* progress bar: always "visible" */
  const s = { vis: true, last: -1, measure() {}, update() {
    const p = clamp(L.sy / Math.max(1, L.doc - L.vh));
    if (Math.abs(p - this.last) > 0.0005) { prog.style.transform = `scaleX(${p})`; this.last = p; }
    return false;
  } };
  scenes.push(s);
}

/* ---------------- hero: WebGL token crystal ---------------- */
const Hero = { morph: 0, spin: 0, pulse: 0, opacity: 1 };
function heroGL() {
  const cv = $('#gl');
  const gl = cv.getContext('webgl', { alpha: true, antialias: false, premultipliedAlpha: true, depth: false, stencil: false, powerPreference: 'low-power' });
  if (!gl) { cv.remove(); return; }
  const small = L.vw < 760;
  const DPR = Math.min(devicePixelRatio || 1, small ? 1.5 : 2);

  /* glyph atlas: 8x8 cells, drawn once into a 2D canvas */
  const GL_TOK = ['•', 'fn', 'let', 'var', '->', 'int', 'str', '{ }', 'ret', 'if', 'for', '( )', '=', '+', '[ ]', 'IR', '%0', 'C', 'JS', ':', 'ok', 'in', '..', 'char', 'bool', '*', 'arena', 'free', 'keep', 'inout', 'print', '"…"', 'E0230', 'while', '0', '1', '2', '5', '25', 'struct'];
  const at = document.createElement('canvas'); at.width = at.height = 512;
  const c2 = at.getContext('2d');
  c2.fillStyle = '#fff'; c2.textAlign = 'center'; c2.textBaseline = 'middle';
  GL_TOK.forEach((t, i) => {
    const cx = (i % 8) * 64 + 32, cy = Math.floor(i / 8) * 64 + 32;
    if (i === 0) {
      const g = c2.createRadialGradient(cx, cy, 0, cx, cy, 14);
      g.addColorStop(0, 'rgba(255,255,255,1)'); g.addColorStop(.35, 'rgba(255,255,255,.85)'); g.addColorStop(1, 'rgba(255,255,255,0)');
      c2.fillStyle = g; c2.fillRect(cx - 16, cy - 16, 32, 32); c2.fillStyle = '#fff'; return;
    }
    let fs = 30; c2.font = `600 ${fs}px ui-monospace, "SF Mono", Menlo, Consolas, monospace`;
    const w = c2.measureText(t).width; if (w > 56) { fs = Math.floor(fs * 56 / w); c2.font = `600 ${fs}px ui-monospace, "SF Mono", Menlo, Consolas, monospace`; }
    c2.fillText(t, cx, cy + 1);
  });

  /* geometry: an icosahedron lattice of tokens, an inner one, and loose dust */
  const P = []; // [x,y,z, t, side, seed, glyph]
  const rnd = (() => { let s = 7; return () => (s = (s * 16807) % 2147483647) / 2147483647; })();
  const tokFor = () => 1 + Math.floor(rnd() * (GL_TOK.length - 1));
  const push = (x, y, z, g) => { P.push([x, y, z, rnd(), rnd() < .5 ? 0 : 1, rnd(), g]); return P.length - 1; };
  const phi = (1 + Math.sqrt(5)) / 2;
  const V = [[-1, phi, 0], [1, phi, 0], [-1, -phi, 0], [1, -phi, 0], [0, -1, phi], [0, 1, phi], [0, -1, -phi], [0, 1, -phi], [phi, 0, -1], [phi, 0, 1], [-phi, 0, -1], [-phi, 0, 1]];
  const E = [];
  for (let i = 0; i < 12; i++) for (let j = i + 1; j < 12; j++) {
    const d = (V[i][0] - V[j][0]) ** 2 + (V[i][1] - V[j][1]) ** 2 + (V[i][2] - V[j][2]) ** 2;
    if (Math.abs(d - 4) < .01) E.push([i, j]);
  }
  const norm = (v, r) => { const l = Math.hypot(v[0], v[1], v[2]); return [v[0] / l * r, v[1] / l * r, v[2] / l * r]; };
  const lines = [];
  const lattice = (r, k, withLines) => {
    const vi = V.map(v => { const p = norm(v, r); return push(p[0], p[1], p[2], 0); });
    E.forEach(([a, b]) => {
      let prev = vi[a];
      for (let s = 1; s <= k; s++) {
        const f = s / (k + 1);
        const p = [V[a][0] + (V[b][0] - V[a][0]) * f, V[a][1] + (V[b][1] - V[a][1]) * f, V[a][2] + (V[b][2] - V[a][2]) * f];
        const q = norm(p, r * (0.93 + 0.07 * Math.abs(f - .5) * 2));
        const id = push(q[0], q[1], q[2], tokFor());
        if (withLines) lines.push(prev, id); prev = id;
      }
      if (withLines) lines.push(prev, vi[b]);
    });
  };
  lattice(1, small ? 2 : 3, true);
  lattice(.5, 1, true);
  for (let i = 0, n = small ? 34 : 70; i < n; i++) {
    const u = rnd() * 2 - 1, a = rnd() * Math.PI * 2, r = 1.35 + rnd() * .7, s = Math.sqrt(1 - u * u);
    push(Math.cos(a) * s * r, u * r, Math.sin(a) * s * r, rnd() < .25 ? 0 : tokFor());
  }
  const N = P.length;
  const pts = new Float32Array(N * 7); P.forEach((p, i) => pts.set(p, i * 7));
  const lin = new Float32Array(lines.length * 7); lines.forEach((id, i) => lin.set(P[id], i * 7));

  const VS = `
attribute vec3 aPos; attribute vec4 aInfo;
uniform mat3 uRot; uniform float uTime, uMorph, uF, uAspect, uDist, uSize, uPulse, uLine; uniform vec2 uShift;
varying float vA; varying float vG; varying vec3 vC;
void main(){
  float seed = aInfo.z;
  vec3 a = aPos * (1.0 + 0.028*sin(uTime*1.3 + seed*6.283) + uPulse*0.42*(0.4 + 0.6*fract(seed*13.7)));
  a = uRot * a;
  float tt = fract(aInfo.x - uTime*0.03);
  float side = aInfo.y*2.0 - 1.0;
  float sp = smoothstep(0.06, 0.4, tt);
  float ang = tt*20.0 + aInfo.y*3.1416 + uTime*0.7;
  vec3 b = vec3(side*0.82*sp + cos(ang)*0.16*(0.35 + sp), 1.3 - tt*3.8, sin(ang)*0.16*(0.35 + sp));
  float m = smoothstep(0.0, 1.0, clamp(uMorph*1.8 - seed*0.8, 0.0, 1.0));
  vec3 p = mix(a, b, m);
  float w = uDist - p.z;
  gl_Position = vec4(p.x*uF/uAspect, p.y*uF, 0.0, w);
  gl_Position.xy += uShift*w;
  float d = clamp((p.z + 1.4)/2.8, 0.0, 1.0);
  gl_PointSize = uSize*(0.5 + 0.85*d)*uDist/w;
  float ends = smoothstep(0.0, 0.07, tt)*(1.0 - smoothstep(0.82, 1.0, tt));
  vA = (0.16 + 0.84*d*d) * mix(1.0, ends, m) * mix(1.0, 1.0 - m, uLine);
  vG = aInfo.w;
  vec3 teal = vec3(0.043, 0.847, 0.714), mint = vec3(0.384, 0.965, 0.71), sky = vec3(0.22, 0.74, 0.97);
  vC = mix(teal, mint, clamp(d*0.9 + 0.25*sin(seed*40.0), 0.0, 1.0));
  vC = mix(vC, aInfo.y < 0.5 ? teal : sky, m*sp);
}`;
  const FS_P = `precision mediump float; uniform sampler2D uTex; varying float vA; varying float vG; varying vec3 vC;
void main(){ vec2 cell = vec2(mod(vG, 8.0), floor(vG/8.0)); float a = texture2D(uTex, (cell + gl_PointCoord)/8.0).a*vA; gl_FragColor = vec4(vC*a, a); }`;
  const FS_L = `precision mediump float; varying float vA; varying float vG; varying vec3 vC;
void main(){ float a = vA*0.22; gl_FragColor = vec4(vC*a, a); }`;
  const sh = (type, src) => { const s = gl.createShader(type); gl.shaderSource(s, src); gl.compileShader(s); if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s)); return s; };
  const prog = (fs) => {
    const p = gl.createProgram(); gl.attachShader(p, sh(gl.VERTEX_SHADER, VS)); gl.attachShader(p, sh(gl.FRAGMENT_SHADER, fs));
    gl.bindAttribLocation(p, 0, 'aPos'); gl.bindAttribLocation(p, 1, 'aInfo'); gl.linkProgram(p);
    if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(p));
    const u = {}; ['uRot', 'uTime', 'uMorph', 'uF', 'uAspect', 'uDist', 'uSize', 'uPulse', 'uLine', 'uShift', 'uTex'].forEach(n => u[n] = gl.getUniformLocation(p, n));
    return { p, u };
  };
  const PP = prog(FS_P), PL = prog(FS_L);
  const buf = (data) => { const b = gl.createBuffer(); gl.bindBuffer(gl.ARRAY_BUFFER, b); gl.bufferData(gl.ARRAY_BUFFER, data, gl.STATIC_DRAW); return b; };
  const bP = buf(pts), bL = buf(lin);
  const tex = gl.createTexture(); gl.bindTexture(gl.TEXTURE_2D, tex);
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, at);
  gl.generateMipmap(gl.TEXTURE_2D);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
  gl.enable(gl.BLEND); gl.blendFunc(gl.ONE, gl.ONE);
  gl.enableVertexAttribArray(0); gl.enableVertexAttribArray(1);

  /* sizing */
  let W = 0, H = 0, F = 1, shift = [0, 0], size = 24, aspect = 1;
  const DIST = 4;
  function resize() {
    const w = cv.clientWidth, h = cv.clientHeight; if (!w || !h) return;
    W = Math.round(w * DPR); H = Math.round(h * DPR);
    if (cv.width !== W || cv.height !== H) { cv.width = W; cv.height = H; }
    gl.viewport(0, 0, W, H); aspect = w / h;
    const wide = w >= 900;
    const R = wide ? Math.min(h * .3, w * .2) : Math.min(w * .3, 140);      // crystal radius in css px
    F = R / (h / 2) * DIST;
    const cx = wide ? w * .74 : w * .5, cy = wide ? h * .5 : 64 + (Math.min(w * .68, 330) - 10) / 2 + 6;
    shift = [cx / w * 2 - 1, 1 - cy / h * 2];
    size = (wide ? 30 : 23) * DPR;
  }
  resize();
  if ('ResizeObserver' in window) new ResizeObserver(() => { resize(); if (RM) draw(); }).observe(cv);
  else addEventListener('resize', () => setTimeout(resize, 130));

  /* interaction */
  let tx = 0, ty = 0, px = 0, py = 0, spinV = 0, lastSy = L.sy;
  if (FINE) addEventListener('pointermove', (e) => { tx = (e.clientX / L.vw - .5) * 2; ty = (e.clientY / L.vh - .5) * 2; }, { passive: true });
  const hero = $('.hero');
  let drag = null;
  hero.addEventListener('pointerdown', (e) => {
    if (e.target.closest('a,button,.stage')) return;
    Hero.pulse = 1; spinV += 1.6; drag = { x: e.clientX, y: e.clientY };
  });
  hero.addEventListener('pointermove', (e) => {
    if (!drag || e.pointerType === 'mouse') return;
    spinV += (e.clientX - drag.x) * 0.01; ty = clamp(ty + (e.clientY - drag.y) * 0.004, -1, 1); drag = { x: e.clientX, y: e.clientY };
  }, { passive: true });
  addEventListener('pointerup', () => { drag = null; }, { passive: true });

  let t = 0, ay = 0.6;
  function draw() {
    const ax = 0.42 + py * 0.35, ayy = ay + px * 0.6;
    const cx = Math.cos(ax), sx = Math.sin(ax), cy = Math.cos(ayy), sy = Math.sin(ayy);
    // R = Ry * Rx (column-major)
    const rot = [cy, 0, -sy, sy * sx, cx, cy * sx, sy * cx, -sx, cy * cx];
    gl.clearColor(0, 0, 0, 0); gl.clear(gl.COLOR_BUFFER_BIT);
    for (const [Pg, b, mode, count, line] of [[PL, bL, gl.LINES, lines.length, 1], [PP, bP, gl.POINTS, N, 0]]) {
      gl.useProgram(Pg.p); const u = Pg.u;
      gl.bindBuffer(gl.ARRAY_BUFFER, b);
      gl.vertexAttribPointer(0, 3, gl.FLOAT, false, 28, 0);
      gl.vertexAttribPointer(1, 4, gl.FLOAT, false, 28, 12);
      gl.uniformMatrix3fv(u.uRot, false, rot);
      gl.uniform1f(u.uTime, t); gl.uniform1f(u.uMorph, Hero.morph); gl.uniform1f(u.uF, F); gl.uniform1f(u.uAspect, aspect);
      gl.uniform1f(u.uDist, DIST); gl.uniform1f(u.uSize, size); gl.uniform1f(u.uPulse, Hero.pulse); gl.uniform1f(u.uLine, line);
      gl.uniform2f(u.uShift, shift[0], shift[1]);
      if (!line) { gl.activeTexture(gl.TEXTURE0); gl.bindTexture(gl.TEXTURE_2D, tex); gl.uniform1i(u.uTex, 0); }
      gl.drawArrays(mode, 0, count);
    }
  }
  if (RM) { Hero.morph = 0; draw(); return; }
  let lost = false;
  cv.addEventListener('webglcontextlost', (e) => { e.preventDefault(); lost = true; });
  function tick(dt) {
    if (lost) return false;
    const s = dt / 1000; t += s;
    const dsy = L.sy - lastSy; lastSy = L.sy;
    spinV += Math.min(Math.abs(dsy) * 0.0016, 0.4);
    spinV *= Math.exp(-s * 2.2);
    ay += s * (0.16 + spinV);
    px = damp(px, tx, dt, 260); py = damp(py, ty, dt, 260);
    Hero.pulse *= Math.exp(-s * 3.2);
    if (Hero.opacity <= 0.01) return true;
    draw();
    return true;
  }
  whileVisible(hero, tick, () => { lastSy = L.sy; });
}

/* hero parallax + stage tilt + canvas fade, all scroll-driven */
function heroScroll() {
  const hero = $('.hero'), inner = $('#heroIn'), stage = $('#stage3d > .stage'), cv = $('#gl');
  let heroH = 1, stageTop = 0, tilt = 1, lastP = -1;
  scene(hero, {
    measure() { heroH = hero.offsetHeight; stageTop = absTop($('#stage3d')); },
    update(dt) {
      const p = L.sy / L.vh;
      if (Math.abs(p - lastP) > 0.0002) {
        lastP = p;
        if (!RM && p < 1.4) inner.style.transform = `translate3d(0, ${(p * L.vh * 0.32).toFixed(1)}px, 0) scale(${(1 - p * 0.05).toFixed(4)})`;
        if (!RM) inner.style.opacity = clamp(1 - p * 1.3).toFixed(3);
        Hero.morph = clamp((p - 0.08) / 0.85);
        Hero.opacity = clamp((heroH - L.sy - L.vh * 0.15) / (L.vh * 0.55));
        if (cv) cv.style.opacity = Hero.opacity.toFixed(3);
      }
      if (RM || !stage) return false;
      const r = (stageTop - L.sy) / L.vh;
      const target = clamp((r - 0.12) / 0.72);
      tilt = damp(tilt, target, dt, 70);
      const e = tilt * tilt;
      stage.style.transform = `translate3d(0, ${(e * 40).toFixed(1)}px, 0) rotateX(${(e * 30).toFixed(2)}deg) scale(${(1 - e * 0.08).toFixed(4)})`;
      return Math.abs(tilt - target) > 0.0005;
    }
  });
}

/* ---------------- token stream ---------------- */
function stream() {
  const A = ['fn', 'main', '(', ')', '{', 'let', 's', '=', '"agents write nyra"', 'for', 'w', 'in', 's.split(" ")', 'print', 'w.upper()', '}', 'struct', 'Rect', 'w: int', 'h: int', 'var', 'r', '+=', '1', 'ret', 'fib(n - 1)', '->', 'int'];
  const B = ['%0: int = call sq(5)', 'print(%0)', 'return (x * x)', 'static int64_t ny_sq', 'function sq(x)', 'E0230', 'hint: write its type', '"ok":true', 'arena { }', 'free(x)', 'keep(x)', 'inout', 'break', 'continue', '[int]', 'str', 'char', 'bool', 'float'];
  const K = /^(fn|let|var|for|in|ret|struct|inout|break|continue|arena)$/, T = /^(int|str|char|bool|float|\[int\])$/;
  const fill = (el, arr) => {
    if (!el) return;
    const html = arr.map(w => `<span class="${K.test(w) ? 'k' : T.test(w) ? 't' : /^E\d/.test(w) ? 'e' : ''}">${esc(w)}</span>`).join('');
    el.innerHTML = html + html;
  };
  fill($('#tok1'), A); fill($('#tok2'), B);
}

/* ---------------- hero demo: agent writes, both backends run ---------------- */
function stageDemo() {
  const root = $('#stage'); if (!root) return;
  const data = JSON.parse($('#hero-data').textContent);
  const code = $('#scode'), ghost = $('#sghost'), file = $('#sfile'), task = $('#stask'), phase = $('#sphase');
  const outs = [$('#sout0'), $('#sout1')], flow = $$('#sflow li'), seg = $('#sseg');
  $$('#sbars i').forEach((b, k) => b.style.setProperty('--k', k));
  /* example picker with a spring knob */
  const knob = document.createElement('span'); knob.className = 'knob'; seg.append(knob);
  const btns = data.map((d, i) => { const b = document.createElement('button'); b.type = 'button'; b.textContent = d.label; b.setAttribute('aria-pressed', 'false'); b.addEventListener('click', () => start(i, true)); seg.append(b); return b; });
  const moveKnob = (i) => { const b = btns[i]; knob.style.width = b.offsetWidth + 'px'; knob.style.transform = `translateX(${b.offsetLeft}px)`; };
  let ex = 0, t = 0, n = -1, plan = null, lines = [], step = 0;
  const setPhase = (txt, cls) => { phase.textContent = txt; phase.className = 'pill pill-mono st-phase ' + (cls || ''); };
  const setFlow = (k) => flow.forEach((li, i) => li.classList.toggle('on', i === k));
  function build(i, final) {
    const d = data[i];
    file.textContent = d.file; task.textContent = d.task;
    ghost.innerHTML = hlLines(d.code);
    lines = [];
    outs.forEach((o, k) => {
      o.innerHTML = `<span class="cmd">$ nyra run ${esc(d.file)}${k ? ' --js' : ''}</span>\n` + d.out.map(s => `<span class="ln">${esc(s)}</span>`).join('\n');
      lines.push($$('.ln', o));
    });
    btns.forEach((b, k) => b.setAttribute('aria-pressed', k === i));
    moveKnob(i);
    if (final) { code.innerHTML = hlLines(d.code); lines.flat().forEach(l => l.classList.add('on')); root.classList.add('matched'); setPhase('identical', 'ok'); setFlow(3); }
  }
  function start(i, user) {
    ex = i; t = 0; n = -1; step = 0; build(i, RM);
    if (RM) return;
    root.classList.remove('matched'); root.classList.add('typing');
    setPhase('writing', 'busy'); setFlow(0);
    const d = data[i], T = d.code.length / 0.05;   // 50 chars per second
    plan = { typeEnd: T, check: T + 350, ok: T + 1100, out: T + 1500, match: T + 1500 + d.out.length * 170 + 350, end: T + 1500 + d.out.length * 170 + 350 + (user ? 5200 : 3600) };
  }
  build(0, true);
  if (RM) return;
  code.setAttribute('aria-hidden', 'true');
  const sr = document.createElement('p'); sr.className = 'sr'; root.append(sr);
  let started = false;
  whileVisible(root, (dt) => {
    if (!plan) return true;
    t += dt;
    const d = data[ex];
    const k = Math.min(d.code.length, Math.floor(t * 0.05));
    if (k !== n) { n = k; code.innerHTML = hlLines(d.code.slice(0, k)).replace(/<\/div>$/, '<span class="caret"></span></div>'); }
    if (step === 0 && t >= plan.typeEnd) { step = 1; root.classList.remove('typing'); }
    if (step === 1 && t >= plan.check) { step = 2; setPhase('checking', 'busy'); setFlow(1); }
    if (step === 2 && t >= plan.ok) { step = 3; setPhase('0 errors', 'ok'); setFlow(2); code.innerHTML = hlLines(d.code); }
    if (step === 3 && t >= plan.out) {
      const j = Math.floor((t - plan.out) / 170);
      lines.forEach((ls) => ls.forEach((l, i) => { if (i <= j) l.classList.add('on'); }));
      if (t >= plan.match) { step = 4; root.classList.add('matched'); setPhase('identical', 'ok'); setFlow(3); sr.textContent = 'Both backends printed: ' + d.out.join(', '); }
    }
    if (step === 4 && t >= plan.end) { step = 0; start((ex + 1) % data.length); }
    return true;
  }, () => { if (!started) { started = true; start(0); } });
}

/* ---------------- tilt cards, glare, magnetic buttons ---------------- */
function tilts() {
  if (!FINE || RM) return;
  const active = new Set();
  $$('.tilt').forEach(card => {
    const g = document.createElement('i'); g.className = 'glare'; g.setAttribute('aria-hidden', 'true'); card.prepend(g);
    const st = { rx: 0, ry: 0, tx: 0, ty: 0, rect: null };
    card._t = st;
    card.addEventListener('pointerenter', () => { st.rect = card.getBoundingClientRect(); card.classList.add('hot'); active.add(card); Loop.add(tick); });
    card.addEventListener('pointermove', (e) => {
      if (!st.rect) return;
      const x = e.clientX - st.rect.left, y = e.clientY - st.rect.top;
      st.ty = (x / st.rect.width - .5) * 9; st.tx = -(y / st.rect.height - .5) * 9;
      g.style.transform = `translate3d(${x}px, ${y}px, 0)`;
    });
    card.addEventListener('pointerleave', () => { st.tx = st.ty = 0; st.rect = null; card.classList.remove('hot'); });
  });
  function tick(dt) {
    active.forEach(card => {
      const s = card._t;
      s.rx = damp(s.rx, s.tx, dt, 110); s.ry = damp(s.ry, s.ty, dt, 110);
      card.style.transform = `perspective(900px) rotateX(${s.rx.toFixed(2)}deg) rotateY(${s.ry.toFixed(2)}deg) translateZ(0)`;
      if (!s.rect && Math.abs(s.rx) < .02 && Math.abs(s.ry) < .02) { card.style.transform = ''; active.delete(card); }
    });
    return active.size > 0;
  }
  $$('.mag').forEach(b => {
    b.addEventListener('pointermove', (e) => { const r = b.getBoundingClientRect(); b.style.transform = `translate(${(e.clientX - r.left - r.width / 2) * .18}px, ${(e.clientY - r.top - r.height / 2) * .3}px)`; });
    b.addEventListener('pointerleave', () => { b.style.transform = ''; });
  });
}

/* ---------------- errors: code, position, hint, fix, run ---------------- */
function errorsDemo() {
  const root = $('#err'), code = $('#ecode'), badge = $('#ebadge');
  const src = ['fn main() {', null, '    xs.push(1)', '    print(xs)', '}'];
  code.innerHTML = src.map(l => l === null
    ? `<div class="l">${hlInline('    var xs')}<span class="ins">${hlInline(': [int]')}</span>${hlInline(' = ')}<span class="sq">${hlInline('[]')}</span></div>`
    : `<div class="l">${hlInline(l)}</div>`).join('');
  const rows = $$('#eterm .row'), jh = $$('#eterm .jh');
  const setB = (txt, cls) => { badge.textContent = txt; badge.className = 'badge ' + (cls || ''); };
  const final = () => { root.classList.add('fixed', 'done'); root.classList.remove('err-on'); rows.forEach(r => r.classList.add('on')); setB('0 errors', 'ok'); };
  if (RM) { final(); return; }
  const reset = () => { root.classList.remove('fixed', 'done', 'err-on'); rows.forEach(r => r.classList.remove('on')); jh.forEach(j => j.classList.remove('hl')); setB('unchecked'); };
  const hl = (k) => jh.forEach(j => j.classList.toggle('hl', j.dataset.k === k));
  const steps = [
    [500, () => rows[0].classList.add('on')],
    [1000, () => { rows[1].classList.add('on'); root.classList.add('err-on'); setB('1 error', 'err'); }],
    [1700, () => hl('code')], [2400, () => hl('pos')], [3100, () => hl('hint')],
    [4100, () => { hl(''); root.classList.remove('err-on'); root.classList.add('fixed'); setB('edited'); }],
    [4900, () => rows[2].classList.add('on')],
    [5400, () => { rows[3].classList.add('on'); setB('0 errors', 'ok'); root.classList.add('done'); }],
    [6100, () => rows[4].classList.add('on')],
    [6500, () => rows[5].classList.add('on')],
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

/* ---------------- compiler pipeline: scroll-driven 3D flight ---------------- */
function pipeline() {
  const flight = $('#flight'); if (!flight || RM) return;
  const cards = $$('.pc', flight).map(el => ({ el, i: +el.dataset.i, x: +(el.dataset.x || 0), near: false, shown: true }));
  const rail = $$('#rail li'), fill = $('#railFill'), grid = $('#grid'), big = $('#bignum');
  $$('.pc .code.hl', flight).forEach(pre => { pre.innerHTML = hlLines(pre.textContent, pre.dataset.lang || 'nyra'); });
  let top = 0, span = 1, s = 0, target = 0, onIdx = -1, wide = false, ch = 0;
  rail.forEach((li, k) => li.querySelector('button').addEventListener('click', () => {
    scrollTo({ top: top + (k / 6) * span + 2, behavior: 'smooth' });
  }));
  function place() {
    const D = wide ? 760 : 560;
    const spread = sstep(4.35, 5.0, s);
    for (const c of cards) {
      const d = c.i - s;
      let op = d >= 0 ? 1 - d * 0.62 : 1 + d * 2.6;
      op = clamp(op);
      const show = op > 0.004;
      if (show !== c.shown) { c.el.style.visibility = show ? 'visible' : 'hidden'; c.shown = show; }
      if (!show) continue;
      let x = 0, y = -d * (wide ? 26 : 40), ry = wide ? d * -7 : 0, rx = d * 4;
      if (c.x) {
        if (wide) { x = c.x * spread * (Math.min(420, L.vw * .3) / 2 + 14); ry += -c.x * (1 - spread) * 10; }
        else { y += c.x * spread * (ch / 2 + 8); x = c.x * (1 - spread) * 6; }
      }
      const z = -d * D;
      c.el.style.transform = `translate(-50%, -50%) translate3d(${x.toFixed(1)}px, ${y.toFixed(1)}px, ${z.toFixed(1)}px) rotateX(${rx.toFixed(2)}deg) rotateY(${ry.toFixed(2)}deg)`;
      c.el.style.opacity = op.toFixed(3);
      const zi = String(Math.round(200 - Math.abs(d) * 20));
      if (zi !== c.zi) { c.zi = zi; c.el.style.zIndex = zi; }
      const near = Math.abs(d) < 0.55;
      if (near !== c.near) { c.near = near; c.el.classList.toggle('near', near); }
    }
    const on = Math.round(s);
    if (on !== onIdx) {
      onIdx = on; rail.forEach((li, k) => li.classList.toggle('on', k === on));
      if (big) { big.textContent = '0' + (on + 1); big.classList.remove('pop'); void big.offsetWidth; big.classList.add('pop'); }
    }
    if (grid) grid.style.transform = `translate3d(0, ${((s * 150) % 60).toFixed(2)}px, 0)`;
    if (fill) fill.style.transform = `scaleY(${(s / 6).toFixed(4)})`;
  }
  scene(flight, {
    measure() {
      top = absTop(flight); span = Math.max(1, flight.offsetHeight - L.vh); wide = L.vw >= 960;
      const c5 = cards.find(c => c.x === 1); ch = c5 ? c5.el.offsetHeight : 200;
    },
    update(dt) {
      target = clamp((L.sy - top) / span) * 6;
      s = damp(s, target, dt, 85);
      if (Math.abs(s - target) < 0.0008) s = target;
      place();
      return s !== target;
    }
  });
}

/* ---------------- memory: copy on write ---------------- */
function cowDemo() {
  const root = $('#cow'); if (!root) return;
  const code = $('#ccode');
  code.innerHTML = hlLines(code.textContent);
  const ln = $$('.l', code);
  const g = (id) => document.getElementById(id);
  const ta = g('c-ta'), tb = g('c-tb'), pa = g('c-pa'), pb1 = g('c-pb1'), pb2 = g('c-pb2'), b1 = g('c-b1'), b2 = g('c-b2'), c3 = g('c-c3'), v0 = g('c-v0'), rc1 = g('c-rc1'), cell0 = g('c-cell0');
  const cout = $('#cout'), cap = $('#ccap'), diag = $('#diag');
  const rc = rc1.parentNode;
  const cur = (...ks) => ln.forEach((l, i) => l.classList.toggle('cur', ks.includes(i)));
  const bump = (el) => { el.classList.remove('bump'); void el.getBBox(); el.classList.add('bump'); };
  if (RM) return;
  root.classList.add('anim');
  const on = (...els) => els.forEach(e => e.classList.add('on'));
  const off = (...els) => els.forEach(e => e.classList.remove('on'));
  const reset = () => { off(ta, tb, pa, pb1, pb2, b1, b2, c3, cout); v0.textContent = '1'; rc1.textContent = 'rc 1'; cur(); cap.textContent = 'Two variables, one array.'; };
  const steps = [
    [500, () => { cur(1); on(ta, b1); setTimeout(() => on(pa), 250); cap.textContent = 'a holds an array of two numbers.'; }],
    [2300, () => { cur(2); on(tb, pb1); rc1.textContent = 'rc 2'; bump(rc); cap.textContent = 'b = a copies nothing yet: both share one block, counted twice.'; }],
    [4300, () => { cur(3); off(pb1); on(b2); rc1.textContent = 'rc 1'; bump(rc); setTimeout(() => on(pb2), 350); setTimeout(() => on(c3), 650); cap.textContent = 'b writes, so the shared block splits: b gets its own copy.'; }],
    [6500, () => { cur(4); cell0.classList.remove('flip'); void cell0.getBBox(); cell0.classList.add('flip'); setTimeout(() => { v0.textContent = '9'; }, 300); cap.textContent = 'a is the only owner now, so it writes in place.'; }],
    [8300, () => { cur(5, 6); on(cout); cap.textContent = 'Two independent values: [9, 2] and [1, 2, 3].'; }],
  ];
  const END = 12500;
  let t = 0, i = 0, rx = 0, ry = 0, tx = 0, ty = 0, rect = null;
  if (FINE) {
    root.addEventListener('pointerenter', () => { rect = diag.getBoundingClientRect(); });
    root.addEventListener('pointermove', (e) => { if (!rect) return; ty = ((e.clientX - rect.left) / rect.width - .5) * 14; tx = -((e.clientY - rect.top) / rect.height - .5) * 10; });
    root.addEventListener('pointerleave', () => { rect = null; tx = ty = 0; });
  }
  reset();
  let clock = 0;
  whileVisible(root, (dt) => {
    t += dt; clock += dt / 1000;
    while (i < steps.length && steps[i][0] <= t) steps[i++][1]();
    if (t >= END) { t = 0; i = 0; reset(); }
    rx = damp(rx, tx, dt, 160); ry = damp(ry, ty, dt, 160);
    const fx = rx + Math.sin(clock * .8) * 2.5 + 4, fy = ry + Math.cos(clock * .6) * 3.5 - 3;
    diag.style.transform = `rotateX(${fx.toFixed(2)}deg) rotateY(${fy.toFixed(2)}deg)`;
    return true;
  });
}

/* ---------------- language tour tabs ---------------- */
function tour() {
  const root = $('#tour'); if (!root) return;
  const tabs = $$('[role=tab]', root), panels = $$('[role=tabpanel]', root), list = $('.tablist', root), wrap = $('.tabs-wrap', root);
  const [il, im, ir] = $$('.ind i', root);
  let cur = 0;
  tabs.forEach((t, i) => {
    t.id = 'tab' + i; panels[i].id = 'tp' + i;
    t.setAttribute('aria-controls', 'tp' + i); panels[i].setAttribute('aria-labelledby', 'tab' + i);
    panels[i].tabIndex = 0;
    t.addEventListener('click', () => sel(i, true));
    t.addEventListener('keydown', (e) => {
      const k = e.key === 'ArrowRight' ? 1 : e.key === 'ArrowLeft' ? -1 : 0;
      if (k) { e.preventDefault(); const j = (cur + k + tabs.length) % tabs.length; sel(j, true); tabs[j].focus(); }
    });
  });
  const ind = (i) => {
    const b = tabs[i], x = b.offsetLeft, w = b.offsetWidth, h = 38;
    il.style.transform = `translateX(${x}px)`;
    im.style.transform = `translateX(${x + h / 2}px) scaleX(${Math.max(0, w - h)})`;
    ir.style.transform = `translateX(${x + w - h}px)`;
  };
  function sel(i, user) {
    cur = i;
    tabs.forEach((t, k) => { t.setAttribute('aria-selected', k === i); t.tabIndex = k === i ? 0 : -1; });
    panels.forEach((p, k) => { const show = k === i; if (show) { p.hidden = false; if (user && !RM) { p.classList.remove('enter'); void p.offsetWidth; p.classList.add('enter'); } } else p.hidden = true; });
    ind(i);
    if (user && wrap.scrollWidth > wrap.clientWidth) wrap.scrollTo({ left: tabs[i].offsetLeft - 40, behavior: RM ? 'auto' : 'smooth' });
  }
  [il, im, ir].forEach(e => e.style.transition = 'none');
  sel(0, false);
  requestAnimationFrame(() => requestAnimationFrame(() => [il, im, ir].forEach(e => e.style.transition = '')));
  addEventListener('resize', () => ind(cur));
}

/* ---------------- highlight static code, copy buttons ---------------- */
function codeBlocks() {
  $$('pre.code.hl').forEach(pre => { if (!pre.closest('#flight')) pre.innerHTML = hlLines(pre.textContent, pre.dataset.lang || 'nyra'); });
  $$('pre[data-copy]').forEach(pre => {
    const host = pre.closest('.win'); if (!host) return;
    const b = document.createElement('button'); b.type = 'button'; b.className = 'copy'; b.textContent = 'copy';
    b.setAttribute('aria-label', 'Copy code');
    b.addEventListener('click', async () => {
      const txt = pre.textContent;
      try { await navigator.clipboard.writeText(txt); } catch (e) {
        const ta = document.createElement('textarea'); ta.value = txt; ta.style.position = 'fixed'; ta.style.opacity = '0'; document.body.append(ta); ta.select();
        try { document.execCommand('copy'); } catch (_) { /* ignore */ } ta.remove();
      }
      b.textContent = 'copied'; b.classList.add('done');
      setTimeout(() => { b.textContent = 'copy'; b.classList.remove('done'); }, 1400);
    });
    host.append(b);
  });
}

/* ---------------- agent loop: a dot travels write, check, fix, check, run ---------------- */
function agentLoop() {
  const svg = $('#loop svg'), path = $('#loopPath'), dot = $('#loopDot'); if (!path) return;
  const nodes = $$('#loop .nd'), X = [80, 240, 400, 560];
  const len = path.getTotalLength(), S = 240, pts = [];
  for (let k = 0; k <= S; k++) { const p = path.getPointAtLength(len * k / S); pts.push([p.x, p.y]); }
  const at = (u) => { const f = clamp(u) * S, k = Math.floor(f), r = f - k, a = pts[k], b = pts[Math.min(S, k + 1)]; return [a[0] + (b[0] - a[0]) * r, a[1] + (b[1] - a[1]) * r]; };
  let lit = -1;
  const show = (u) => {
    const [x, y] = at(u);
    dot.setAttribute('transform', `translate(${x.toFixed(1)} ${y.toFixed(1)})`);
    let n = -1; X.forEach((nx, i) => { if (Math.abs(x - nx) < 46 && Math.abs(y - 60) < 18) n = i; });
    if (n !== lit) { lit = n; nodes.forEach((g, i) => g.classList.toggle('lit', i === n)); }
  };
  if (RM) { show(1); return; }
  const DUR = 6400; let t = 0;
  whileVisible(svg, (dt) => {
    t = (t + dt) % (DUR + 900);
    const u = t / DUR;
    show(Math.min(1, u));
    return true;
  });
}

/* ---------------- roadmap: line fills as you read ---------------- */
function roadmap() {
  const road = $('#road'), fill = $('#rf'); if (!road || !fill) return;
  let top = 0, h = 1, v = 0;
  scene(road, {
    measure() { top = absTop(road); h = road.offsetHeight; },
    update(dt) {
      const target = clamp((L.sy + L.vh * 0.62 - top) / h);
      v = RM ? target : damp(v, target, dt, 90);
      fill.style.transform = `scaleY(${v.toFixed(4)})`;
      return Math.abs(v - target) > 0.0005;
    }
  });
}

/* ---------------- 3D logo tile ---------------- */
function tile() {
  const el = $('#tile'); if (!el || RM) return;
  let t = 0, px = 0, py = 0, tx = 0, ty = 0;
  if (FINE) addEventListener('pointermove', (e) => { tx = (e.clientX / L.vw - .5) * 40; ty = (e.clientY / L.vh - .5) * 30; }, { passive: true });
  whileVisible(el, (dt) => {
    t += dt / 1000; px = damp(px, tx, dt, 200); py = damp(py, ty, dt, 200);
    el.style.transform = `rotateX(${(16 + Math.sin(t * .9) * 10 - py).toFixed(2)}deg) rotateY(${(Math.sin(t * .55) * 34 + px).toFixed(2)}deg) translateY(${(Math.sin(t * 1.4) * 5).toFixed(2)}px)`;
    return true;
  });
}

/* pause decorative CSS loops while their sheet is off screen */
function pauseOffscreen() {
  const po = new IntersectionObserver((es) => es.forEach(e => e.target.classList.toggle('paused', !e.isIntersecting)));
  $$('.bento').forEach(b => po.observe(b));
}

/* ---------------- boot ---------------- */
safe('reveals', reveals);
safe('header', header);
safe('stream', stream);
safe('code', codeBlocks);
safe('heroScroll', heroScroll);
safe('heroGL', heroGL);
safe('stage', stageDemo);
safe('tilts', tilts);
safe('errors', errorsDemo);
safe('pipeline', pipeline);
safe('cow', cowDemo);
safe('tour', tour);
safe('loop', agentLoop);
safe('roadmap', roadmap);
safe('tile', tile);
safe('pause', pauseOffscreen);
measure();
addEventListener('load', measure);
if ('ResizeObserver' in window) { let first = true; new ResizeObserver(() => { if (first) { first = false; return; } clearTimeout(rsT); rsT = setTimeout(measure, 100); }).observe($('main')); }
})();
