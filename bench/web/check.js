// node check.js URL SPEC.json OUT_PREFIX  -> prints one JSON object (and writes OUT_PREFIX.jpg, a full-page screenshot)
//
// The functional checklist of the web A/B test, identical for the HTML and the Nyra version of a site: headless
// Brave at 1280x900 (desktop only), the browser time zone and clock fixed by the spec (CDP
// Emulation.setTimezoneOverride and a Date that starts at the spec's instant), and every check driven by the brief's
// own text, never by class names or markup of either side. Items are pass/fail; the score is passed / total.
//
// Passes: A (normal motion: everything except the two below), B (the "closed" clock, only when the brief has a live
// badge), C (prefers-reduced-motion: reduce). Console errors and CSP violations are collected over all passes.
const fs = require('fs');
const { launch, sleep } = require('./cdp');

const [url, specPath, outPrefix] = process.argv.slice(2);
const spec = JSON.parse(fs.readFileSync(specPath, 'utf8'));

// ------------------------------------------------------------------ helpers that run in the page
const HELPERS = String.raw`
window.__wb = (() => {
  const norm = (s) => String(s || '').normalize('NFC')
    .replace(/[    ]/g, ' ').replace(/[‘’ʼ]/g, "'").replace(/[“”„]/g, '"')
    .replace(/[–—−]/g, '-').replace(/\s*-\s*(?=\d)/g, '-').replace(/…/g, '...')
    .replace(/(\d)[ ,](?=\d{3}(?!\d))/g, '$1').replace(/\s+%/g, '%').replace(/\s+/g, ' ').trim().toLowerCase();
  const SKIP = new Set(['SCRIPT', 'STYLE', 'NOSCRIPT', 'TEMPLATE', 'svg', 'SVG']);
  const textOf = (el) => {
    let out = '';
    const walk = (n) => {
      if (n.nodeType === 3) { out += n.nodeValue; return; }
      if (n.nodeType !== 1 || SKIP.has(n.nodeName)) return;
      if (n.nodeName === 'BR') out += ' ';
      for (const c of n.childNodes) walk(c);
      if (/^(P|DIV|LI|TD|TH|TR|H[1-6]|SECTION|ARTICLE|BUTTON|A|LABEL|SPAN|DT|DD|FIGCAPTION|BLOCKQUOTE|SUMMARY)$/.test(n.nodeName)) out += ' ';
    };
    walk(el);
    return out;
  };
  const clipped = (el) => {
    const r = el.getBoundingClientRect();
    let a = el.parentElement;
    while (a && a !== document.body && a !== document.documentElement) {
      const s = getComputedStyle(a);
      if (/(hidden|clip|auto|scroll)/.test(s.overflowX + s.overflowY)) {
        const q = a.getBoundingClientRect();
        const w = Math.min(r.right, q.right) - Math.max(r.left, q.left), h = Math.min(r.bottom, q.bottom) - Math.max(r.top, q.top);
        if (w < 2 || h < 2) return true;
      }
      a = a.parentElement;
    }
    return false;
  };
  const visible = (el) => {
    if (!el || !el.isConnected) return false;
    const r = el.getBoundingClientRect();
    if (r.width < 2 || r.height < 2) return false;
    if (el.checkVisibility && !el.checkVisibility({ opacityProperty: true, visibilityProperty: true, contentVisibilityAuto: true })) return false;
    if (getComputedStyle(el).visibility === 'hidden') return false;
    return !clipped(el);
  };
  const root = (id) => (id ? document.getElementById(id) : null) || document.body;
  // the deepest elements whose text contains s (normalized)
  const deepest = (s, scope) => {
    const want = norm(s);
    const all = [...root(scope).querySelectorAll('*')].filter((e) => !SKIP.has(e.nodeName) && !e.closest('script,style,template,noscript'));
    const hits = all.filter((e) => norm(textOf(e)).includes(want));
    return hits.filter((e) => !hits.some((o) => o !== e && e.contains(o))).sort((a, b) => norm(textOf(a)).length - norm(textOf(b)).length);
  };
  const present = (s, scope) => norm(textOf(root(scope))).includes(norm(s));
  const shows = (s, scope) => deepest(s, scope).some(visible);
  // like shows, but s must stand as whole words ("Reception open" is not found in "Reception · Opening hours")
  const showsWord = (s, scope) => {
    const re = new RegExp('(^|[^\\p{L}\\p{N}])' + norm(s).replace(/[.*+?^$()|[\]\\]/g, '\\$&') + '(?![\\p{L}\\p{N}])', 'u');
    return deepest(s, scope).filter((e) => re.test(norm(textOf(e)))).some(visible);
  };
  const tokenRe = (v) => new RegExp('(^|[^0-9])' + norm(v).replace(/[.*+?^$()|[\]\\]/g, '\\$&') + '([^0-9]|$)');
  const showsNumber = (v, scope) => {
    const re = tokenRe(v);
    const all = [...root(scope).querySelectorAll('*')].filter((e) => !SKIP.has(e.nodeName));
    const hits = all.filter((e) => re.test(norm(textOf(e))));
    return hits.filter((e) => !hits.some((o) => o !== e && e.contains(o))).some(visible);
  };
  const clickable = (el) => el.closest('button,a,summary,label,[role=button],[role=tab],[role=switch],[onclick],[aria-expanded],[tabindex]') || el;
  // a control whose own (short) text matches one of the names, preferring exact matches
  const control = (names, scope) => {
    const want = names.map(norm);
    const els = [...root(scope).querySelectorAll('button,a,label,summary,[role=button],[role=tab],[role=switch],[role=radio],input[type=radio],input[type=checkbox],span,div,li')];
    const label = (e) => norm(e.getAttribute('aria-label') || textOf(e) || e.value || '');
    for (const exact of [true, false]) {
      const m = els.filter((e) => { const t = label(e); return t.length < 40 && want.some((w) => exact ? t === w : t.startsWith(w)); })
        .filter((e) => visible(e) || (e.matches('input') && e.labels && [...e.labels].some(visible)));
      if (m.length) { const b = m.find((e) => e.matches('button,a,[role=button],[role=tab],[role=radio],label')) || m[0]; return clickable(b); }
    }
    return null;
  };
  const scrollThrough = async (step = 400, wait = 90) => {
    for (let y = 0; y < document.documentElement.scrollHeight; y += step) { window.scrollTo(0, y); await new Promise((r) => setTimeout(r, wait)); }
    window.scrollTo(0, document.documentElement.scrollHeight);
    await new Promise((r) => setTimeout(r, 300));
  };
  const scrollSection = async (id) => {
    const s = document.getElementById(id); if (!s) return;
    const top = s.getBoundingClientRect().top + scrollY - 80, h = s.getBoundingClientRect().height;
    for (let y = top; y <= top + h; y += 300) { window.scrollTo(0, y); await new Promise((r) => setTimeout(r, 110)); }
    window.scrollTo(0, top); await new Promise((r) => setTimeout(r, 250));
  };
  const labelOf = (c) => {
    const parts = [];
    if (c.labels) for (const l of c.labels) if (visible(l)) parts.push(textOf(l));
    const lb = c.getAttribute('aria-labelledby');
    if (lb) for (const id of lb.split(/\s+/)) { const e = document.getElementById(id); if (e && visible(e)) parts.push(textOf(e)); }
    return norm(parts.join(' ')).replace(/-/g, '');
  };
  return { norm, textOf, visible, deepest, present, shows, showsWord, showsNumber, control, clickable, scrollThrough, scrollSection, labelOf, root, tokenRe };
})();
`;

function fakeClock(iso) {
  const at = Date.parse(iso);
  return `(() => { const RD = Date, start = RD.now(), base = ${at};
    function D(...a) { if (!new.target) return new D().toString(); return a.length ? new RD(...a) : new RD(base + (RD.now() - start)); }
    D.prototype = RD.prototype; D.now = () => base + (RD.now() - start); D.parse = RD.parse; D.UTC = RD.UTC;
    Object.setPrototypeOf(D, RD); window.Date = D; })();`;
}

const PROBE = `window.__csp = []; document.addEventListener('securitypolicyviolation', (e) => window.__csp.push(e.violatedDirective + ' ' + (e.blockedURI || '')), true);`;

(async () => {
  const b = await launch();
  const errors = [], csp = new Set(), navs = [], badImages = [];
  const reqType = new Map();
  b.on('Runtime.exceptionThrown', (p) => errors.push('exception: ' + (p.exceptionDetails.exception?.description || p.exceptionDetails.text).split('\n')[0].slice(0, 200)));
  b.on('Runtime.consoleAPICalled', (p) => { if (p.type === 'error') errors.push('console.error: ' + p.args.map((a) => a.value ?? a.description).join(' ').slice(0, 200)); });
  b.on('Log.entryAdded', (p) => {
    const e = p.entry; if (e.level !== 'error') return;
    if (/favicon/.test(e.url || '') || /favicon/.test(e.text)) return;
    if (/Content Security Policy/i.test(e.text)) { csp.add(e.text.replace(/\s+/g, ' ').slice(0, 220)); return; }
    errors.push('log: ' + e.text.slice(0, 200) + (e.url ? ' (' + e.url.slice(0, 120) + ')' : ''));
  });
  b.on('Page.frameRequestedNavigation', (p) => navs.push(p.url));
  b.on('Page.windowOpen', (p) => navs.push(p.url));
  b.on('Network.requestWillBeSent', (p) => reqType.set(p.requestId, { type: p.type, url: p.request.url }));
  b.on('Network.responseReceived', (p) => { if (p.type === 'Image' && p.response.status >= 400 && !/favicon/.test(p.response.url)) badImages.push(p.response.status + ' ' + p.response.url.slice(0, 120)); });
  b.on('Network.loadingFailed', (p) => {
    const r = reqType.get(p.requestId);
    if (r && r.type === 'Image' && !/favicon/.test(r.url) && !p.canceled) badImages.push((p.blockedReason || p.errorText) + ' ' + r.url.slice(0, 120));
  });
  const anims = [];
  b.on('Animation.animationStarted', (p) => anims.push(p.animation));

  await b.send('Page.enable'); await b.send('Runtime.enable'); await b.send('Log.enable'); await b.send('Network.enable');
  await b.send('Network.setCacheDisabled', { cacheDisabled: true });
  await b.send('Emulation.setDeviceMetricsOverride', { width: 1280, height: 900, deviceScaleFactor: 1, mobile: false });
  await b.send('Emulation.setTimezoneOverride', { timezoneId: spec.clock.tz });
  await b.send('Emulation.setLocaleOverride', { locale: 'en-US' });
  await b.send('Page.addScriptToEvaluateOnNewDocument', { source: PROBE });
  let clockScript = (await b.send('Page.addScriptToEvaluateOnNewDocument', { source: fakeClock(spec.clock.at) })).identifier;

  const items = [];
  let visibleA = [];
  const item = (id, name, pass, detail) => items.push({ id, name, pass: !!pass, detail: detail || '' });
  const ev = (expr, t) => b.eval(expr, t);
  const load = async () => {
    let loaded = false; const done = () => { loaded = true; };
    b.on('Page.loadEventFired', done);
    await b.send('Page.navigate', { url });
    for (let i = 0; i < 150 && !loaded; i++) await sleep(100);
    await sleep(1200);
    await ev(HELPERS);
  };
  const safe = async (id, name, fn) => {
    try { await fn(); } catch (e) { item(id, name, false, 'checker error: ' + String(e.message || e).slice(0, 200)); }
  };

  // ============================================================ pass A
  await load();
  await ev('__wb.scrollThrough()', 60000);
  await sleep(2500);
  // images (lazy ones have been scrolled past; wait up to 10 s for the slow ones)
  for (let i = 0; i < 20; i++) {
    const pending = await ev(`[...document.images].filter(i => __wb.visible(i) && i.currentSrc && !i.complete).length`);
    if (!pending) break; await sleep(500);
  }
  await ev('window.scrollTo(0, 0)'); await sleep(800);
  // screenshot of the whole page (half scale, JPEG)
  try {
    const m = await b.send('Page.getLayoutMetrics');
    const h = Math.min(Math.ceil(m.cssContentSize.height), 16000);
    const shot = await b.send('Page.captureScreenshot', { format: 'jpeg', quality: 62, captureBeyondViewport: true,
      clip: { x: 0, y: 0, width: 1280, height: h, scale: 0.5 } }, 60000);
    fs.writeFileSync(outPrefix + '.jpg', Buffer.from(shot.data, 'base64'));
  } catch (e) { errors.push('checker: screenshot failed ' + e.message); }

  await safe('title', 'page title and description', async () => {
    const r = await ev(`({t: __wb.norm(document.title), d: __wb.norm((document.querySelector('meta[name=description]')||{}).content)})`);
    const okT = r.t === (await ev(`__wb.norm(${JSON.stringify(spec.title)})`));
    const okD = r.d === (await ev(`__wb.norm(${JSON.stringify(spec.description)})`));
    item('title', 'page title and description', okT && okD, [okT ? '' : 'title: ' + r.t, okD ? '' : 'description: ' + r.d].filter(Boolean).join('; '));
  });

  await safe('text', 'all brief text present and visible', async () => {
    const r = await ev(`(() => { const S = ${JSON.stringify(spec.text)}, H = ${JSON.stringify(spec.hidden_text || [])};
      const missing = [...S, ...H].filter(s => !__wb.present(s)); const hidden = S.filter(s => __wb.present(s) && !__wb.shows(s));
      return { missing, hidden, total: S.length + H.length }; })()`, 60000);
    const still = [];
    for (const s of r.hidden.slice(0, 30)) {
      await ev(`(() => { const e = __wb.deepest(${JSON.stringify(s)})[0]; if (e) e.scrollIntoView({block: 'center'}); })()`);
      await sleep(700);
      if (!(await ev(`__wb.shows(${JSON.stringify(s)})`))) still.push(s);
    }
    r.hidden = still.concat(r.hidden.slice(30));
    visibleA = spec.text.filter((s) => !r.missing.includes(s) && !r.hidden.includes(s));
    item('text', 'all brief text present and visible', !r.missing.length && !r.hidden.length,
      (r.missing.length ? 'missing: ' + r.missing.join(' | ') : '') + (r.hidden.length ? (r.missing.length ? '; ' : '') + 'not visible: ' + r.hidden.join(' | ') : ''));
  });

  await safe('images', 'all brief images present and loaded', async () => {
    const r = await ev(`(() => { const want = ${JSON.stringify(spec.images)};
      const imgs = [...document.images];
      const bgs = [...document.querySelectorAll('*')].map(e => getComputedStyle(e).backgroundImage).filter(x => x && x !== 'none').join(' ');
      const missing = want.filter(w => !imgs.some(i => (i.currentSrc + ' ' + i.src + ' ' + (i.getAttribute('srcset')||'')).includes('/seed/' + w.seed + '/')
          || __wb.norm(i.alt) === __wb.norm(w.alt)) && !bgs.includes('/seed/' + w.seed + '/')).map(w => w.seed);
      const broken = imgs.filter(i => __wb.visible(i) && i.getAttribute('src') !== null && (!i.complete || !i.naturalWidth)).map(i => (i.currentSrc || i.src).slice(0, 100));
      const noAlt = imgs.filter(i => !i.hasAttribute('alt')).length;
      return { missing, broken, noAlt }; })()`);
    const bad = [...new Set(badImages)];
    item('images', 'all brief images present and loaded', !r.missing.length && !r.broken.length && !bad.length,
      [r.missing.length ? 'missing: ' + r.missing.join(', ') : '', r.broken.length ? 'not loaded: ' + r.broken.join(', ') : '',
        bad.length ? 'failed requests: ' + bad.slice(0, 5).join(', ') : ''].filter(Boolean).join('; '));
  });

  await safe('hscroll', 'no horizontal scroll at 1280 px', async () => {
    const r = await ev(`({sw: document.documentElement.scrollWidth, bw: document.body.scrollWidth, w: window.innerWidth})`);
    const ok = Math.max(r.sw, r.bw) <= r.w + 1;
    item('hscroll', 'no horizontal scroll at 1280 px', ok, ok ? '' : `scrollWidth ${Math.max(r.sw, r.bw)} > ${r.w}`);
  });

  await safe('links', 'phone, e-mail and map links', async () => {
    const r = await ev(`(() => { const L = ${JSON.stringify(spec.links)}; const as = [...document.querySelectorAll('a[href]')];
      return L.map(l => {
        if (l.href_prefix === 'tel:') return as.some(a => a.getAttribute('href').startsWith('tel:') && a.getAttribute('href').replace(/\\D/g, '').endsWith(l.digits.slice(-9))) ? '' : 'no tel: link';
        if (l.href_prefix === 'mailto:') return as.some(a => a.getAttribute('href').toLowerCase().startsWith('mailto:' + l.address)) ? '' : 'no mailto: link';
        return as.some(a => __wb.norm(__wb.textOf(a)).includes(__wb.norm(l.text)) && l.href_any.some(h => decodeURI(a.href).includes(h))) ? '' : 'no "' + l.text + '" link';
      }).filter(Boolean); })()`);
    item('links', 'phone, e-mail and map links', !r.length, r.join('; '));
  });

  if (spec.map) await safe('map', 'map embed shows', async () => {
    const r = await ev(`(() => { const f = [...document.querySelectorAll('iframe')].find(f => (f.getAttribute('src')||'').includes(${JSON.stringify(spec.map.src)}));
      if (!f) return { found: false }; f.scrollIntoView(); const q = f.getBoundingClientRect(); return { found: true, w: q.width, h: q.height, vis: __wb.visible(f) }; })()`);
    await sleep(2500);
    const tree = await b.send('Page.getFrameTree');
    const kids = (tree.frameTree.childFrames || []).map((c) => c.frame.url);
    const loaded = kids.some((u) => u.includes(spec.map.host));
    item('map', 'map embed shows', r.found && r.vis && r.w >= 200 && r.h >= 150 && loaded,
      !r.found ? 'no iframe with the map URL' : `size ${Math.round(r.w)}x${Math.round(r.h)}, visible ${r.vis}, frame loaded ${loaded} (${kids.join(', ').slice(0, 120)})`);
  });

  if (spec.counters) await safe('counters', 'counters end on the final number', async () => {
    const C = spec.counters;
    await ev(`(() => { const e = __wb.deepest(${JSON.stringify(C[0].label)})[0]; if (e) e.scrollIntoView({block: 'center'}); })()`);
    await sleep(4000);
    const r = await ev(countersExpr(C));
    item('counters', 'counters end on the final number', !r.length, r.join('; '));
  });

  if (spec.badge) await safe('badge_open', 'live badge says "' + spec.badge.open + '" while open', async () => {
    await ev('window.scrollTo(0, 0)'); await sleep(400);
    const r = await ev(`({open: __wb.showsWord(${JSON.stringify(spec.badge.open)}), closed: __wb.showsWord(${JSON.stringify(spec.badge.closed)})})`);
    item('badge_open', 'live badge says "' + spec.badge.open + '" while open', r.open && !r.closed, `"${spec.badge.open}" visible ${r.open}, "${spec.badge.closed}" visible ${r.closed}`);
  });

  if (spec.badge && spec.badge.today_row) await safe('today', "today's row highlighted", async () => {
    const r = await ev(`(() => { const B = ${JSON.stringify(spec.badge)};
      const row = (day) => { const c = __wb.deepest(day, B.section).filter(__wb.visible)[0]; if (!c) return null;
        let e = c; while (e.parentElement && !/\\d{1,2}[:.]\\d{2}|closed/i.test(__wb.textOf(e)) ) e = e.parentElement; return e; };
      let a = row(B.today_row), o = row(B.other_row); if (!a || !o) return { ok: false, why: 'no row for ' + (a ? B.other_row : B.today_row) };
      if (a === o) { const cell = (day) => __wb.deepest(day, B.section).filter(__wb.visible)[0];
        const wrap = (c) => { const d = document.createElement('div'); return [c, c.nextElementSibling].filter(Boolean); };
        const sigs = (xs) => xs.map(x => { const s = getComputedStyle(x); return [s.backgroundColor, s.color, s.fontWeight, s.borderTopColor + s.borderTopWidth, s.boxShadow].join('|'); }).join('#');
        const ca = cell(B.today_row), co = cell(B.other_row);
        return { ok: sigs(wrap(ca)) !== sigs(wrap(co)) || ca.getAttribute('aria-current') !== null, why: 'the row of ' + B.today_row + ' looks like the row of ' + B.other_row }; }
      const sig = (e) => { const out = []; for (const x of [e, ...e.querySelectorAll('*')]) { const s = getComputedStyle(x);
        out.push([s.backgroundColor, s.color, s.fontWeight, s.borderTopColor + s.borderTopWidth, s.borderLeftColor + s.borderLeftWidth, s.boxShadow, s.outlineStyle, s.backgroundImage].join('|')); }
        const bf = getComputedStyle(e, '::before'), af = getComputedStyle(e, '::after'); out.push(bf.content + bf.backgroundColor, af.content + af.backgroundColor); return out.join('#'); };
      return { ok: sig(a) !== sig(o) || a.getAttribute('aria-current') !== null, why: 'the row of ' + B.today_row + ' looks like the row of ' + B.other_row }; })()`);
    item('today', "today's row highlighted", r.ok, r.ok ? '' : r.why);
  });

  // nav: every link scrolls to an existing section
  await safe('nav', 'every nav link scrolls to its section', async () => {
    const fails = [];
    for (const n of spec.nav) {
      const id = n.href.slice(1);
      await ev('window.scrollTo(0, 0)'); await sleep(500);
      const r = await ev(`(() => { const want = ${JSON.stringify(n.label)}, id = ${JSON.stringify(id)};
        if (!document.getElementById(id)) return 'no element with id ' + id;
        const as = [...document.querySelectorAll('a[href]')].filter(a => { const h = a.getAttribute('href'); return h === '#' + id || h.endsWith('/#' + id) || h.endsWith('.html#' + id); })
          .filter(a => __wb.norm(__wb.textOf(a)).includes(__wb.norm(want)) || __wb.norm(a.getAttribute('aria-label')).includes(__wb.norm(want)));
        const top = as.filter(a => __wb.visible(a) && a.getBoundingClientRect().top < 160);
        if (!top.length) return as.length ? 'link not visible in the top bar' : 'no link';
        top[0].click(); return ''; })()`);
      if (r) { fails.push(n.label + ': ' + r); continue; }
      await sleep(1600);
      const pos = await ev(`(() => { const t = document.getElementById(${JSON.stringify(id)}).getBoundingClientRect().top;
        const atEnd = scrollY + innerHeight >= document.documentElement.scrollHeight - 4; return { t, atEnd }; })()`);
      if (!((pos.t >= -40 && pos.t <= 260) || (pos.atEnd && pos.t < 900 && pos.t >= -40))) fails.push(`${n.label}: section top at ${Math.round(pos.t)} px after the click`);
    }
    item('nav', 'every nav link scrolls to its section', !fails.length, fails.join('; '));
  });

  if (spec.faq) await safe('faq', 'FAQ items open and close', async () => {
    const fails = [];
    await ev(`__wb.scrollSection(${JSON.stringify(spec.faq.section)})`, 30000);
    for (const f of spec.faq.items) {
      const q = JSON.stringify(f.q), a = JSON.stringify(f.a), sec = JSON.stringify(spec.faq.section);
      const before = await ev(`__wb.shows(${a}, ${sec})`);
      // the question itself, or else a button of the same item ("Show answer"), is what a visitor clicks
      const target = `(() => { const qs = ${JSON.stringify(spec.faq.items.map((x) => x.q))}; const e = __wb.deepest(${q}, ${sec}).filter(__wb.visible)[0]; if (!e) return null;
        const c = __wb.clickable(e); if (c !== e) return c;
        for (let p = e.parentElement, k = 0; p && k < 4; p = p.parentElement, k++) {
          if (qs.filter(x => __wb.norm(__wb.textOf(p)).includes(__wb.norm(x))).length > 1) break;
          const b = [...p.querySelectorAll('button,[role=button],summary,a[href="#"]')].find(__wb.visible); if (b) return b; }
        return e; })()`;
      const clicked = await ev(`(() => { const t = ${target}; if (!t) return false; t.scrollIntoView({block: 'center'}); t.click(); return true; })()`);
      if (!clicked) { fails.push(`"${f.q}" not found`); continue; }
      await sleep(800);
      const open = await ev(`__wb.shows(${a}, ${sec})`);
      await ev(`(() => { const t = ${target}; if (t) t.click(); })()`);
      await sleep(800);
      const closed = !(await ev(`__wb.shows(${a}, ${sec})`));
      if (before || !open || !closed) fails.push(`"${f.q}": shown before ${before}, after click ${open}, hidden after 2nd click ${closed}`);
    }
    item('faq', 'FAQ items open and close', !fails.length, fails.join('; '));
  });

  // the filter items carry their group in `day` (gym, round 3) or `group` (later briefs); the item name may be set by the spec
  if (spec.filter) await safe('filter', spec.filter.name || 'day filter shows the right classes', async () => {
    const F = spec.filter, sec = JSON.stringify(F.section), NAME = F.name || 'day filter shows the right classes';
    for (const it of F.items) if (it.day === undefined) it.day = it.group;
    const state = async () => ev(`(() => ${JSON.stringify(F.items)}.map(i => __wb.shows(i.name, ${sec})))()`);
    const fails = [];
    await ev(`__wb.scrollSection(${sec})`, 30000);
    const init = await state();
    if (init.some((v) => !v)) fails.push('on load not all classes visible: hidden ' + F.items.filter((_, k) => !init[k]).map((i) => i.name).join(', '));
    for (const t of F.try) {
      const ok = await ev(`(() => { const c = __wb.control(${JSON.stringify([t.button, t.full])}, ${sec}); if (!c) return false; c.click(); return true; })()`);
      if (!ok) { fails.push(`no "${t.button}" button`); continue; }
      await sleep(700);
      await ev(`__wb.scrollSection(${sec})`, 30000);
      await sleep(500);
      const s = await state();
      const wrong = F.items.filter((it, k) => s[k] !== (t.button === 'All' || it.day === t.button)).map((it, k) => it.name);
      if (wrong.length) fails.push(`${t.button}: wrong visibility of ${wrong.join(', ')}`);
    }
    item('filter', NAME, !fails.length, fails.join('; '));
  });

  // tabs (round 4): on load the first tab's panel shows and no other; each tab click shows its panel and hides the rest
  if (spec.tabs) await safe('tabs', spec.tabs.name || 'tabs show the right panel', async () => {
    const T = spec.tabs, sec = JSON.stringify(T.section), NAME = T.name || 'tabs show the right panel';
    const state = async () => ev(`(() => ${JSON.stringify(T.tabs)}.map(t => t.texts.map(s => __wb.shows(s, ${sec}))))()`);
    const judge = (s, k) => T.tabs.map((t, j) => {
      const bad = t.texts.filter((_, i) => s[j][i] !== (j === k));
      return bad.length ? `${j === k ? 'not shown' : 'shown'}: ${t.button} (${bad.length}/${t.texts.length} texts)` : '';
    }).filter(Boolean);
    const fails = [];
    await ev(`__wb.scrollSection(${sec})`, 30000);
    const init = judge(await state(), 0);
    if (init.length) fails.push('on load ' + init.join(', '));
    for (const name of T.try) {
      const k = T.tabs.findIndex((t) => t.button === name);
      const ok = await ev(`(() => { const c = __wb.control(${JSON.stringify([name])}, ${sec}); if (!c) return false; c.scrollIntoView({block: 'center'}); c.click(); return true; })()`);
      if (!ok) { fails.push(`no "${name}" tab`); continue; }
      await sleep(900);
      await ev(`__wb.scrollSection(${sec})`, 30000);
      await sleep(300);
      const w = judge(await state(), k);
      if (w.length) fails.push(`after "${name}": ` + w.join(', '));
    }
    item('tabs', NAME, !fails.length, fails.join('; '));
  });

  // a price calculator (round 4): pick a size (select, radio or button), tick services (checkbox or toggle button),
  // and the expected total must show in the section while the previous total no longer does
  if (spec.calculator) await safe('calc', spec.calculator.name || 'calculator shows the right total', async () => {
    const C = spec.calculator, sec = JSON.stringify(C.section), NAME = C.name || 'calculator shows the right total';
    const fails = [];
    await ev(`__wb.scrollSection(${sec})`, 30000);
    const choose = (name) => ev(`(() => { const name = ${JSON.stringify(name)}, sc = __wb.root(${sec}), want = __wb.norm(name);
      for (const s of sc.querySelectorAll('select')) { const o = [...s.options].find(o => __wb.norm(o.textContent).startsWith(want));
        if (o) { Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(s, o.value);
          s.dispatchEvent(new Event('input', { bubbles: true })); s.dispatchEvent(new Event('change', { bubbles: true })); return 'select'; } }
      const r = [...sc.querySelectorAll('input[type=radio]')].find(r => __wb.labelOf(r).startsWith(want.replace(/-/g, '')));
      if (r) { if (!r.checked) r.click(); return 'radio'; }
      const c = __wb.control([name], ${sec}); if (!c) return ''; c.click(); return 'control'; })()`);
    const tick = (name, on) => ev(`(() => { const name = ${JSON.stringify(name)}, on = ${on}, sc = __wb.root(${sec}), want = __wb.norm(name).replace(/-/g, '');
      const cb = [...sc.querySelectorAll('input[type=checkbox]')].find(c => __wb.labelOf(c).includes(want));
      if (cb) { if (cb.checked !== on) cb.click(); return 'checkbox'; }
      // a toggle button may mark its state with a leading symbol ("✓ Oil service · 1,790 Kč"): compare without it
      const lab = (e) => __wb.norm(e.getAttribute('aria-label') || __wb.textOf(e) || '').replace(/^[^\\p{L}\\p{N}]+/u, '').replace(/-/g, '');
      const c = __wb.control([name], ${sec}) || [...sc.querySelectorAll('button,[role=button],[role=switch],[role=checkbox],label')]
        .filter(__wb.visible).find(e => { const t = lab(e); return t.length < 60 && t.startsWith(want); });
      if (!c) return '';
      const st = c.getAttribute('aria-pressed') ?? c.getAttribute('aria-checked') ?? c.getAttribute('aria-selected');
      if (st !== null && (st === 'true') === on) return 'already';
      c.click(); return 'control'; })()`);
    const shows = (v) => ev(`__wb.showsNumber(${JSON.stringify(v)}, ${sec})`);
    for (const s of C.steps) {
      const miss = [];
      if (s.choose && !(await choose(s.choose))) miss.push(`no "${s.choose}" choice`);
      for (const n of s.untick || []) if (!(await tick(n, false))) miss.push(`no "${n}" to untick`);
      for (const n of s.tick || []) if (!(await tick(n, true))) miss.push(`no "${n}" to tick`);
      await sleep(700);
      const okE = await shows(s.expect), still = [];
      for (const g of s.gone || []) if (await shows(g)) still.push(g);
      if (miss.length || !okE || still.length) {
        fails.push(`${s.do}: ` + [miss.join(', '), okE ? '' : `${s.expect} not shown`, still.length ? `${still.join(', ')} still shown` : ''].filter(Boolean).join('; '));
        break;  // later steps build on this one
      }
    }
    item('calc', NAME, !fails.length, fails.join('; '));
  });

  if (spec.toggle) await safe('toggle', 'monthly/yearly switch changes the prices', async () => {
    const T = spec.toggle, sec = JSON.stringify(T.section);
    const st = async () => ev(`(() => ({ a: ${JSON.stringify(T.a.prices)}.map(p => __wb.showsNumber(p, ${sec})), b: ${JSON.stringify(T.b.prices)}.map(p => __wb.showsNumber(p, ${sec})) }))()`);
    const fails = [];
    await ev(`__wb.scrollSection(${sec})`, 30000);
    let s = await st();
    if (!s.a.every(Boolean) || s.b.some(Boolean)) fails.push(`on load: monthly shown ${s.a}, yearly shown ${s.b}`);
    const flip = async (name) => {
      const before = JSON.stringify(await st());
      await ev(`(() => { const c = __wb.control(${JSON.stringify([name])}, ${sec}); if (c) c.click(); })()`);
      await sleep(700);
      if (JSON.stringify(await st()) === before) { // the word may be a plain label next to a switch: use the switch
        await ev(`(() => { const sc = __wb.root(${sec});
          const s = sc.querySelector('input[type=checkbox],[role=switch],button[aria-pressed],button[aria-checked]')
            || [...sc.querySelectorAll('button,[role=button],label,input,div,span')].filter(e => __wb.visible(e) || (e.labels && [...e.labels].some(__wb.visible)))
                 .filter(e => /toggle|switch|year|annual|billing|period|month/i.test((e.getAttribute('aria-label') || '') + ' ' + (typeof e.className === 'string' ? e.className : '') + ' ' + e.id) && !__wb.textOf(e).trim())
                 .sort((a, b) => (b.matches('button,input,[role=button]') - a.matches('button,input,[role=button]')))[0];
          if (s) s.click(); })()`);
        await sleep(700);
      }
    };
    await flip(T.b.button); s = await st();
    if (!s.b.every(Boolean) || s.a.some(Boolean)) fails.push(`after "${T.b.button}": monthly shown ${s.a}, yearly shown ${s.b}`);
    await flip(T.a.button); s = await st();
    if (!s.a.every(Boolean) || s.b.some(Boolean)) fails.push(`after "${T.a.button}": monthly shown ${s.a}, yearly shown ${s.b}`);
    item('toggle', 'monthly/yearly switch changes the prices', !fails.length, fails.join('; '));
  });

  if (spec.lightbox) {
    const L = spec.lightbox, sec = JSON.stringify(L.section), imgs = JSON.stringify(L.images);
    // which gallery image the lightbox shows: a visible image inside a fixed overlay or an open dialog
    const current = async () => ev(`(() => { const W = ${imgs};
      const over = [...document.querySelectorAll('img')].filter(i => __wb.visible(i) && (() => { for (let e = i; e; e = e.parentElement) { const s = getComputedStyle(e);
        if (s.position === 'fixed' || (e.tagName === 'DIALOG' && e.open)) return true; } return false; })());
      for (const i of over) { const k = W.findIndex(w => (i.currentSrc + ' ' + i.src).includes('/seed/' + w.seed + '/') || __wb.norm(i.alt) === __wb.norm(w.alt));
        if (k >= 0 && i.getBoundingClientRect().width > 300) return k; }
      return -1; })()`);
    const open = async (k) => ev(`(() => { const w = ${imgs}[${k}]; __wb.root(${sec}).scrollIntoView();
      const fixed = (i) => { for (let e = i; e; e = e.parentElement) { const s = getComputedStyle(e); if (s.position === 'fixed' || (e.tagName === 'DIALOG' && e.open)) return true; } return false; };
      const i = [...__wb.root(${sec}).querySelectorAll('img')].filter(i => __wb.visible(i) && !fixed(i)).find(i => (i.currentSrc + ' ' + i.src).includes('/seed/' + w.seed + '/') || __wb.norm(i.alt) === __wb.norm(w.alt));
      if (!i) return false; __wb.clickable(i).click(); return true; })()`);
    const press = async (key, code, kc) => {
      await b.send('Input.dispatchKeyEvent', { type: 'keyDown', key, code, windowsVirtualKeyCode: kc });
      await b.send('Input.dispatchKeyEvent', { type: 'keyUp', key, code, windowsVirtualKeyCode: kc });
      await sleep(700);
    };
    const btn = async (re) => ev(`(() => { const re = ${re}; const W = ${imgs};
      const isFixed = (x) => { const s = getComputedStyle(x); return s.position === 'fixed' || (x.tagName === 'DIALOG' && x.open); };
      const img = [...document.querySelectorAll('img')].filter(__wb.visible).find(i => W.some(w => (i.currentSrc + ' ' + i.src).includes('/seed/' + w.seed + '/') || __wb.norm(i.alt) === __wb.norm(w.alt)) && (() => { for (let e = i; e; e = e.parentElement) if (isFixed(e)) return true; return false; })());
      let overlay = null; if (img) for (let e = img; e; e = e.parentElement) if (isFixed(e)) overlay = e;
      const label = (e) => (e.getAttribute('aria-label') || '') + ' ' + (e.getAttribute('title') || '') + ' ' + __wb.textOf(e).trim() + ' ' + [...e.querySelectorAll('use')].map(u => u.getAttribute('href') || u.getAttribute('xlink:href') || '').join(' ');
      const pool = overlay ? [...overlay.querySelectorAll('button,a,[role=button]')] : [];
      const all = [...document.querySelectorAll('button,a,[role=button]')].filter(e => !e.closest('header,nav') && (() => { for (let x = e; x; x = x.parentElement) if (isFixed(x)) return true; return false; })());
      const c = pool.filter(__wb.visible).find(e => re.test(label(e))) || all.filter(__wb.visible).find(e => re.test(label(e)));
      if (!c) return false; c.click(); return true; })()`);
    let opened = false;
    await safe('lb_open', 'lightbox opens on click', async () => {
      await ev(`__wb.scrollSection(${sec})`, 30000);
      const ok = await open(0); await sleep(900);
      const k = await current(); opened = k === 0;
      item('lb_open', 'lightbox opens on click', opened, !ok ? 'no gallery image found' : `lightbox shows image ${k}`);
    });
    await safe('lb_nav', 'lightbox next/previous buttons', async () => {
      if (!opened) { await open(0); await sleep(900); }
      const n = await btn('/next|›|❯|→|»|forward|další|chevron-right|arrow-right/i'); await sleep(700); const k1 = await current();
      const p = await btn('/prev|‹|❮|←|«|back|předchozí|chevron-left|arrow-left/i'); await sleep(700); const k2 = await current();
      item('lb_nav', 'lightbox next/previous buttons', n && p && k1 === 1 && k2 === 0, `next button ${n} -> image ${k1}, previous button ${p} -> image ${k2}`);
    });
    await safe('lb_close', 'lightbox closes with its button', async () => {
      if ((await current()) < 0) { await open(0); await sleep(900); }
      const c = await btn('/close|×|✕|✖|zavřít|^\\s*x\\s*$/i'); await sleep(800); const k = await current();
      item('lb_close', 'lightbox closes with its button', c && k < 0, `close button ${c}, lightbox still showing image ${k}`);
    });
    await safe('lb_keys', 'lightbox keyboard: arrows move, Escape closes', async () => {
      await press('Escape', 'Escape', 27);
      await open(0); await sleep(900); const k0 = await current();
      await press('ArrowRight', 'ArrowRight', 39); const k1 = await current();
      await press('ArrowLeft', 'ArrowLeft', 37); const k2 = await current();
      await press('Escape', 'Escape', 27); const k3 = await current();
      item('lb_keys', 'lightbox keyboard: arrows move, Escape closes', k0 === 0 && k1 === 1 && k2 === 0 && k3 < 0,
        `open ${k0}, ArrowRight ${k1}, ArrowLeft ${k2}, Escape ${k3 < 0 ? 'closed' : 'still open (' + k3 + ')'}`);
    });
  }

  if (spec.form) {
    const F = spec.form, sec = JSON.stringify(F.section);
    await safe('form_fields', 'form fields with visible labels (required ones required)', async () => {
      const r = await ev(`(() => { const F = ${JSON.stringify(F)}; const scope = __wb.root(F.section);
        const ctl = [...scope.querySelectorAll('input,select,textarea')].filter(c => !/^(hidden|submit|button|reset|image)$/.test(c.type));
        return F.fields.map(f => { const want = __wb.norm(f.label).replace(/-/g, '');
          const c = ctl.find(c => __wb.labelOf(c).includes(want));
          if (!c) { const any = ctl.find(c => __wb.norm((c.getAttribute('aria-label') || '') + ' ' + (c.placeholder || '') + ' ' + (c.name || '')).replace(/-/g, '').includes(want));
            return any ? f.label + ': no visible label' : f.label + ': no field'; }
          if (f.required && !(c.required || c.getAttribute('aria-required') === 'true')) return f.label + ': not required';
          if (f.options) { const o = [...c.querySelectorAll('option')].map(o => __wb.norm(o.textContent)); const miss = f.options.filter(x => !o.includes(__wb.norm(x)));
            if (miss.length) return f.label + ': options missing ' + miss.join(', '); }
          return ''; }).filter(Boolean); })()`);
      item('form_fields', 'form fields with visible labels (required ones required)', !r.length, r.join('; '));
    });
    await safe('form_send', 'form opens a filled e-mail (mailto)', async () => {
      navs.length = 0;
      const r = await ev(`(() => { const F = ${JSON.stringify(F)}; const scope = __wb.root(F.section);
        const ctl = [...scope.querySelectorAll('input,select,textarea')].filter(c => !/^(hidden|submit|button|reset|image)$/.test(c.type));
        const set = (c, v) => { const proto = c.tagName === 'SELECT' ? HTMLSelectElement.prototype : c.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
          Object.getOwnPropertyDescriptor(proto, 'value').set.call(c, v); c.dispatchEvent(new Event('input', { bubbles: true })); c.dispatchEvent(new Event('change', { bubbles: true })); };
        const missing = [], optv = {};
        for (const f of F.fields) { const want = __wb.norm(f.label).replace(/-/g, '');
          const c = ctl.find(c => __wb.labelOf(c).includes(want)) || ctl.find(c => __wb.norm((c.getAttribute('aria-label') || '') + ' ' + (c.placeholder || '') + ' ' + (c.name || '')).replace(/-/g, '').includes(want));
          if (!c) { missing.push(f.label); continue; }
          if (c.tagName === 'SELECT') { const o = [...c.options].find(o => __wb.norm(o.textContent) === __wb.norm(f.value)); if (o) { set(c, o.value); optv[f.value] = o.value; } else missing.push(f.label + ' option'); }
          else set(c, f.value); }
        // other required controls (a consent checkbox the brief did not ask for) are filled too, so they do not block the test
        for (const c of ctl) if (c.required && c.type === 'checkbox' && !c.checked) c.click();
        const form = (ctl[0] && ctl[0].form) || scope.querySelector('form');
        const btns = [...(form || scope).querySelectorAll('button,input[type=submit]')].filter(__wb.visible);
        const sub = btns.filter(x => x.type === 'submit');
        const b = sub.find(x => __wb.norm(x.value || __wb.textOf(x)).includes(__wb.norm(F.submit))) || sub[0] || btns.find(x => __wb.norm(x.value || __wb.textOf(x)).includes(__wb.norm(F.submit)));
        if (!b) return { missing, clicked: false };
        b.scrollIntoView({ block: 'center' }); b.click(); return { missing, optv, clicked: true, invalid: form ? !form.checkValidity() : null }; })()`);
      await sleep(1500);
      const mails = navs.filter((u) => /^mailto:/i.test(u));
      let why = '';
      if (!r.clicked) why = 'no submit button';
      else if (!mails.length) why = 'no mailto: navigation after submit' + (r.invalid ? ' (the browser found the form invalid)' : '') + (navs.length ? ' (navigated to ' + navs[0].slice(0, 80) + ')' : '');
      else {
        const m = mails[mails.length - 1];
        const q = m.indexOf('?'), addr = q < 0 ? m.slice(7) : m.slice(7, q), query = q < 0 ? '' : m.slice(q + 1);  // like a mail client
        const params = {};
        for (const part of query.split('&')) { const [k, v = ''] = part.split('='); try { params[k.toLowerCase()] = decodeURIComponent(v); } catch (e) { params[k.toLowerCase()] = v; } }
        const body = params.body || '';
        const n = (s) => s.normalize('NFC').toLowerCase();
        const lacks = F.fields.filter((f) => f.in_body !== false).map((f) => f.value)
          .filter((v) => !n(body).includes(n(v)) && !(r.optv && r.optv[v] && n(body).includes(n(r.optv[v]))));
        if (decodeURIComponent(addr).toLowerCase() !== F.mailto) why = 'mailto address ' + addr;
        else if (!n(params.subject || '').includes(n(F.subject))) why = 'subject "' + (params.subject || '') + '"';
        else if (lacks.length) why = 'body lacks ' + lacks.join(', ') + ' (body: ' + JSON.stringify(body.slice(0, 160)) + ')';
      }
      if (r.missing && r.missing.length && !why) why = 'could not fill ' + r.missing.join(', ');
      if (why && mails.length) why += ' [' + mails.length + ' mailto navigation(s), last: ' + mails[mails.length - 1].slice(0, 160) + ']';
      item('form_send', 'form opens a filled e-mail (mailto)', !why, why);
    });
  }

  // ============================================================ pass B: the "closed" clock
  if (spec.badge) await safe('badge_closed', 'live badge says "' + spec.badge.closed + '" while closed', async () => {
    await b.send('Page.removeScriptToEvaluateOnNewDocument', { identifier: clockScript });
    clockScript = (await b.send('Page.addScriptToEvaluateOnNewDocument', { source: fakeClock(spec.badge.closed_at) })).identifier;
    await load();
    await sleep(800);
    const r = await ev(`({open: __wb.showsWord(${JSON.stringify(spec.badge.open)}), closed: __wb.showsWord(${JSON.stringify(spec.badge.closed)})})`);
    item('badge_closed', 'live badge says "' + spec.badge.closed + '" while closed', r.closed && !r.open, `"${spec.badge.closed}" visible ${r.closed}, "${spec.badge.open}" visible ${r.open}`);
  });

  // ============================================================ pass C: reduced motion
  await safe('motion', 'respects prefers-reduced-motion (no animation over 10 ms)', async () => {
    await b.send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-reduced-motion', value: 'reduce' }] });
    await b.send('Animation.enable');
    anims.length = 0;
    await load();
    await ev('__wb.scrollThrough(300, 120)', 60000);
    let counterWhy = [];
    if (spec.counters) {
      await ev('window.scrollTo(0, 0)'); await sleep(300);
      await ev(`(() => { const e = __wb.deepest(${JSON.stringify(spec.counters[0].label)})[0]; if (e) e.scrollIntoView({block: 'center'}); })()`);
      await sleep(150);
      counterWhy = await ev(countersExpr(spec.counters));
    }
    await sleep(800);
    // content that was visible with normal motion must stay visible (a reveal that only an animation undoes is a bug)
    let gone = await ev(`${JSON.stringify(visibleA)}.filter(s => !__wb.shows(s))`, 60000);
    const still = [];
    for (const s of gone.slice(0, 12)) {
      await ev(`(() => { const e = __wb.deepest(${JSON.stringify(s)})[0]; if (e) e.scrollIntoView({block: 'center'}); })()`);
      await sleep(400);
      if (!(await ev(`__wb.shows(${JSON.stringify(s)})`))) still.push(s);
    }
    gone = still.concat(gone.slice(12));
    const long = [];
    for (const a of anims) {
      const d = a.source ? a.source.duration : 0;
      if (d > 10) long.push(`${a.type} ${a.name || a.cssId || ''} ${Math.round(d)} ms`.replace(/\s+/g, ' '));
    }
    const uniq = [...new Set(long)];
    item('motion', 'respects prefers-reduced-motion (no animation over 10 ms)', !uniq.length && !counterWhy.length && !gone.length,
      [uniq.length ? `${long.length} animations, e.g. ${uniq.slice(0, 4).join(', ')}` : '', counterWhy.length ? 'counters not final at once: ' + counterWhy.join(', ') : '',
        gone.length ? 'invisible with reduced motion: ' + gone.slice(0, 6).join(' | ') : ''].filter(Boolean).join('; '));
  });

  // errors and CSP over all passes
  try { const more = await ev('window.__csp || []'); for (const c of more) csp.add('violation: ' + c); } catch (e) { }
  const realErrors = errors.filter((e) => !/^checker/.test(e));
  item('console', 'no console errors', !realErrors.length, realErrors.slice(0, 5).join(' | '));
  item('csp', 'no CSP violations', !csp.size, [...csp].slice(0, 4).join(' | '));

  await b.close();
  const passed = items.filter((i) => i.pass).length;
  console.log(JSON.stringify({ url, score: passed / items.length, passed, total: items.length, items,
    console_errors: realErrors, csp_violations: [...csp], checker_notes: errors.filter((e) => /^checker/.test(e)) }));
  process.exit(0);
})().catch((e) => { console.error(e); process.exit(1); });

function countersExpr(C) {
  return `(() => { const C = ${JSON.stringify(C)}; const out = [];
    for (const c of C) { const lab = __wb.deepest(c.label).filter(__wb.visible).filter(e => __wb.norm(__wb.textOf(e)).length < 80); if (!lab.length) { out.push('no visible "' + c.label + '"'); continue; }
      let ok = false; const re = __wb.tokenRe(c.value);
      for (const l of lab) { let e = l;
        for (let k = 0; k < 5 && e && !ok; k++, e = e.parentElement) { const t = __wb.norm(__wb.textOf(e));
          if (C.some(o => o !== c && t.includes(__wb.norm(o.label)) && !__wb.norm(c.label).includes(__wb.norm(o.label)))) break;
          if (re.test(t)) { ok = e.checkVisibility ? [...e.querySelectorAll('*'), e].some(x => re.test(__wb.norm(__wb.textOf(x))) && __wb.visible(x)) : true; if (ok) break; } } if (ok) break; }
      if (!ok) out.push(c.label + ' shows ' + JSON.stringify(__wb.norm(__wb.textOf(lab[0].parentElement)).slice(0, 40))); }
    return out; })()`;
}
