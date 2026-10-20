// Renders every motion fixture in headless Chrome over the DevTools protocol (no packages) and checks:
//   - normal motion: every `data-anim` element on screen is revealed and fully visible after its
//     entrance, scrolling reveals the rest, counters end on their text, and there are no script errors;
//   - `prefers-reduced-motion: reduce` (and `data-motion="off"`): no animation or transition longer
//     than 10 ms, even under the pointer, and everything is in place at once.
// With a folder as the second argument it also saves frames of each fixture over time, and one
// contact sheet per fixture (sheet-<name>.png), to look at.
//
//   node tests/web/motion/check.mjs <chrome> [frames-folder]
import { spawn } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync, mkdirSync, readdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const [chrome, shots] = process.argv.slice(2);
if (!chrome) { console.error('usage: node check.mjs <chrome> [frames-folder]'); process.exit(2); }
const only = process.env.MOTION_ONLY;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const W = 1200, H = 760;

const profile = mkdtempSync(join(tmpdir(), 'nyra-motion-'));
// (the page must keep rendering at full speed: a headless window the system counts as hidden or occluded,
// as on a macOS CI runner, would otherwise get its frames, timers and so its animations throttled)
const proc = spawn(chrome, ['--headless=new', '--remote-debugging-port=0', `--user-data-dir=${profile}`, '--no-first-run',
  '--no-default-browser-check', '--hide-scrollbars', '--allow-file-access-from-files', '--force-device-scale-factor=1',
  '--disable-background-timer-throttling', '--disable-backgrounding-occluded-windows', '--disable-renderer-backgrounding', 'about:blank'],
  { stdio: 'ignore' });
let port;
for (let i = 0; i < 200 && !port; i++) {
  await sleep(50);
  try { port = readFileSync(join(profile, 'DevToolsActivePort'), 'utf8').split('\n')[0]; } catch {}
}
if (!port) { console.error('Chrome did not start'); proc.kill(); process.exit(2); }
const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
const ws = new WebSocket(targets.find((t) => t.type === 'page').webSocketDebuggerUrl);
await new Promise((r) => (ws.onopen = r));
let seq = 0;
const pending = new Map(), listeners = new Set();
ws.onmessage = (m) => {
  const d = JSON.parse(m.data);
  if (d.id && pending.has(d.id)) {
    const p = pending.get(d.id);
    pending.delete(d.id);
    d.error ? p.rej(new Error(d.error.message)) : p.res(d.result);
  } else if (d.method) for (const f of listeners) f(d);
};
const send = (method, params = {}) => new Promise((res, rej) => {
  const id = ++seq;
  pending.set(id, { res, rej });
  ws.send(JSON.stringify({ id, method, params }));
});
const run = async (expression) => {
  const r = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
  if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text);
  return r.result.value;
};
let errors = [];
listeners.add((d) => {
  if (d.method === 'Runtime.exceptionThrown') errors.push(d.params.exceptionDetails.exception?.description ?? d.params.exceptionDetails.text);
  if (d.method === 'Runtime.consoleAPICalled' && d.params.type === 'error') errors.push(d.params.args.map((a) => a.value ?? a.description).join(' '));
});
await send('Page.enable');
// a page in front and focused renders every frame (a CI runner's headless window may count as hidden)
await send('Page.bringToFront');
await send('Emulation.setFocusEmulationEnabled', { enabled: true });
await send('Runtime.enable');
await send('Emulation.setDeviceMetricsOverride', { width: W, height: H, deviceScaleFactor: 1, mobile: false });

const open = async (url) => {
  errors = [];
  const loaded = new Promise((r) => { const f = (d) => { if (d.method === 'Page.loadEventFired') { listeners.delete(f); r(); } }; listeners.add(f); });
  await send('Page.navigate', { url });
  await loaded;
  // the timing below starts at the first frame (a cold browser on a busy runner can take a while to draw one)
  await run('new Promise(r => { requestAnimationFrame(() => requestAnimationFrame(r)); setTimeout(r, 5000); })');
};
const shot = async (file) => {
  const { data } = await send('Page.captureScreenshot', { format: 'png' });
  writeFileSync(file, Buffer.from(data, 'base64'));
};
const mouse = (x, y) => send('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y });
const center = (sel, n = 0) => run(`(() => { const e = document.querySelectorAll(${JSON.stringify(sel)})[${n}]; e.scrollIntoView({block:'center'}); const r = e.getBoundingClientRect(); return [r.left + r.width / 2, r.top + r.height / 2, r.left, r.top, r.width, r.height]; })()`);

// in the page: what is wrong with motion now (normal pass)
const REVEALED = `(() => {
  const bad = [];
  const onScreen = (e) => { const r = e.getBoundingClientRect(); return r.bottom > 0 && r.top < innerHeight * .9 && r.width > 0; };
  for (const e of document.querySelectorAll('[data-anim]')) {
    if (!onScreen(e)) continue;
    if (!e.classList.contains('m-in')) bad.push('not revealed: ' + e.outerHTML.slice(0, 80));
    const s = getComputedStyle(e);
    if (+s.opacity < .99 && !e.closest('[data-motion=off]')) bad.push('not visible (opacity ' + s.opacity + '): ' + e.outerHTML.slice(0, 80));
  }
  // a scroll word follows the page's scrolling, not a box that never scrolls (overflow: hidden)
  for (const a of document.getAnimations())
    if (a.timeline && a.timeline.source !== undefined && a.timeline.source !== document.scrollingElement)
      bad.push('scroll timeline of ' + a.animationName + ' follows ' + (a.timeline.source?.outerHTML ?? 'nothing').slice(0, 60));
  for (const e of document.querySelectorAll('[data-expect]'))
    if (e.textContent.trim() !== e.dataset.expect) bad.push('counter ended on ' + JSON.stringify(e.textContent) + ', not ' + e.dataset.expect);
  return bad;
})()`;
// in the page: anything moving longer than 10 ms (reduced-motion pass)
const STILL = `(() => {
  const bad = [];
  for (const a of document.getAnimations()) {
    const t = a.effect.getComputedTiming();
    if (t.activeDuration > 10 && a.playState !== 'finished') bad.push('animation ' + (a.animationName || a.transitionProperty || '?') + ' of ' + t.activeDuration + ' ms on ' + (a.effect.target?.outerHTML ?? '').slice(0, 60));
  }
  const secs = (v) => Math.max(...v.split(',').map((x) => parseFloat(x) * (x.trim().endsWith('ms') ? 1 : 1000)));
  for (const e of document.querySelectorAll('*'))
    for (const p of [null, '::before', '::after']) {
      const s = getComputedStyle(e, p);
      const tr = secs(s.transitionDuration), an = s.animationName !== 'none' ? secs(s.animationDuration) : 0;
      if (tr > 10 || an > 10) bad.push((tr > 10 ? 'transition ' + tr : 'animation ' + an) + ' ms on ' + e.tagName.toLowerCase() + (p ?? '') + ' ' + (e.getAttribute('data-anim') ?? e.getAttribute('data-hover') ?? e.getAttribute('data-loop') ?? e.className));
    }
  for (const e of document.querySelectorAll('[data-anim], [data-anim~=stagger] > *, .m-wi, .m-li')) {
    const s = getComputedStyle(e);
    if (+s.opacity < 1 || s.translate !== 'none' && s.translate !== '0px') bad.push('not in place: ' + e.outerHTML.slice(0, 60));
  }
  for (const e of document.querySelectorAll('[data-expect]'))
    if (e.textContent.trim() !== e.dataset.expect) bad.push('counter shows ' + JSON.stringify(e.textContent));
  return bad;
})()`;

const files = readdirSync(here).filter((f) => f.endsWith('.html') && (!only || f.includes(only))).sort();
if (shots) mkdirSync(shots, { recursive: true });
const failures = [];
for (const f of files) {
  const name = f.replace(/\.html$/, '');
  const url = pathToFileURL(join(here, f)).href;
  const html = readFileSync(join(here, f), 'utf8');
  const kind = /name="motion-check" content="(\w+)"/.exec(html)[1];
  const off = /<html[^>]*data-motion="off"/.test(html);
  const fail = (what) => failures.push(`${name}: ${what}`);

  // normal motion
  await send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-reduced-motion', value: 'no-preference' }] });
  await mouse(1, 1);
  await open(url);
  const t0 = Date.now();
  const frames = [];
  const frame = async (label) => {
    if (!shots) return;
    const file = `${name}-${frames.length}.png`;
    await shot(join(shots, file));
    frames.push([file, label]);
  };
  // frames of CSS motion are exact: every running animation (and transition) is paused and set to
  // the same time since its start, so the screenshot's own time does not blur them
  const seek = async (times, label) => {
    if (!shots) return;
    await run('window.__m = document.getAnimations().filter(a => a.timeline == document.timeline); __m.forEach(a => a.pause())');
    for (const ms of times) { await run(`__m.forEach(a => { a.currentTime = ${ms}; })`); await frame(`${label}${ms} ms`); }
    await run('__m.forEach(a => a.play())');
  };
  if (kind === 'enter') {
    await sleep(120);
    await seek([0, 150, 300, 500, 750, 1100, 1600, 2500], '');
  } else if (kind === 'hover') {
    await sleep(600);
    const [x, y, l, t, w, h] = await center('[data-hover]', name === 'hover-lift' ? 1 : 0);
    await frame('before');
    if (name === 'hover-tilt' || name === 'hover-magnetic') {
      const path = [[l + w * .8, t + h * .25], [l + w * .9, t + h * .15], [l + w * .25, t + h * .8], [l + w * .1, t + h * .9]];
      for (const [px, py] of path) { await mouse(px, py); await sleep(220); await frame(`pointer at ${Math.round(px - l)},${Math.round(py - t)}`); }
      await mouse(x, H - 5); await sleep(120); await frame('left +120 ms'); await sleep(500); await frame('left +620 ms');
    } else {
      await mouse(x, y);
      await sleep(20);
      await seek([0, 80, 160, 300, 500, 900], 'hover +');
      await sleep(1000);
      await mouse(x, H - 5);
      await sleep(20);
      await seek([150, 400], 'left +');
    }
  } else if (kind === 'loop') {
    await sleep(400);
    await seek([0, 300, 600, 900, 1200, 1600, 2000, 3000], 'loop ');
  } else if (kind === 'scroll') {
    await sleep(300);
    const max = await run('document.documentElement.scrollHeight - innerHeight');
    for (const p of [0, .2, .4, .6, .8, 1]) { await run(`scrollTo(0, ${Math.round(max * p)})`); await sleep(250); await frame(`scroll ${Math.round(p * 100)}%`); }
    await run('scrollTo(0, 0)');
    await sleep(300);
  }
  // the observer reveals on a rendering step, which a busy machine can be late with: wait (up to 4 s)
  // until every element on screen is revealed, so the check below sees the end of the entrances
  await run(`new Promise(r => { const t = Date.now(), f = () => [...document.querySelectorAll('[data-anim]')].every(e => { const b = e.getBoundingClientRect(); return e.classList.contains('m-in') || !(b.bottom > 0 && b.top < innerHeight * .9 && b.width > 0); }) || Date.now() - t > 4000 ? r() : setTimeout(f, 50); f(); })`);
  // let the entrances that are still running (pausing them for the frames delayed some) finish
  await run(`Promise.race([Promise.all(document.getAnimations().filter(a => a.timeline == document.timeline && isFinite(a.effect.getComputedTiming().endTime)).map(a => a.finished.catch(() => 0))), new Promise(r => setTimeout(r, 4000))])`);
  // counters run on the frame loop: give them their 1.6 s
  await run(`new Promise(r => { const t = Date.now(), f = () => [...document.querySelectorAll('[data-expect]')].every(e => e.textContent.trim() === e.dataset.expect) || Date.now() - t > 3500 ? r() : setTimeout(f, 50); f(); })`);
  if (!off) for (const b of await run(REVEALED)) fail(b);
  // scrolling down reveals the rest
  await run(`(async () => { for (let y = 0; y <= document.documentElement.scrollHeight; y += innerHeight / 2) { scrollTo(0, y); await new Promise(r => setTimeout(r, 120)); } })()`);
  await sleep(200);
  await run(`Promise.race([Promise.all(document.getAnimations().filter(a => a.timeline == document.timeline && isFinite(a.effect.getComputedTiming().endTime)).map(a => a.finished.catch(() => 0))), new Promise(r => setTimeout(r, 4000))])`);
  const hidden = await run(`[...document.querySelectorAll('[data-anim]')].filter(e => !e.classList.contains('m-in')).map(e => e.outerHTML.slice(0, 70))`);
  for (const h of hidden) fail('never revealed after scrolling: ' + h);
  if (off) for (const b of await run(STILL)) fail('motion off: ' + b);
  for (const e of errors) fail('script error: ' + e);

  // reduced motion
  await send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-reduced-motion', value: 'reduce' }] });
  await open(url);
  await sleep(150);
  const hov = await run(`document.querySelector('[data-hover]') ? 1 : 0`);
  if (hov) { const [x, y] = await center('[data-hover]'); await mouse(x, y); await sleep(60); }
  for (const b of await run(STILL)) fail('reduced motion: ' + b);
  for (const e of errors) fail('script error (reduced motion): ' + e);

  if (shots && frames.length) {
    const cells = frames.map(([file, label]) => `<figure><img src="${file}"><figcaption>${label}</figcaption></figure>`).join('');
    writeFileSync(join(shots, `sheet-${name}.html`), `<!doctype html><meta charset="utf-8"><style>body{margin:0;padding:12px;background:#222;color:#ddd;font:13px system-ui}h1{font-size:15px;margin:0 0 8px}main{display:grid;grid-template-columns:repeat(4,1fr);gap:8px}figure{margin:0}img{width:100%;display:block;border:1px solid #444}figcaption{padding:2px 0}</style><h1>${name}</h1><main>${cells}</main>`);
    await send('Emulation.setEmulatedMedia', { features: [] });
    await open(pathToFileURL(join(shots, `sheet-${name}.html`)).href);
    const h = await run('document.documentElement.scrollHeight');
    await send('Emulation.setDeviceMetricsOverride', { width: W, height: h, deviceScaleFactor: 1, mobile: false });
    await sleep(100);
    await shot(join(shots, `sheet-${name}.png`));
    await send('Emulation.setDeviceMetricsOverride', { width: W, height: H, deviceScaleFactor: 1, mobile: false });
  }
  console.log(`${failures.some((x) => x.startsWith(name + ':')) ? 'FAIL' : 'ok  '} ${name}`);
}

ws.close();
proc.kill();
await sleep(300);
try { rmSync(profile, { recursive: true, force: true }); } catch {}
if (failures.length) {
  console.error(`\n${failures.length} problem(s):\n` + failures.map((x) => '  ' + x).join('\n'));
  process.exit(1);
}
console.log(`\nall ${files.length} motion fixtures pass`);
