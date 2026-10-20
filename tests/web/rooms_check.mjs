// Clicks through `examples/web/rooms.nyra` (its built fixture, tests/web/fixtures/rooms) in a headless
// Chrome over the DevTools protocol (no packages):
// - its live view re-renders images with `ratio: "4/3"` and a `grid(8)`: after every click the
//   images keep their 4:3 shape (the photos are 16:10), the grid has 8 columns, and the page's
//   Content-Security-Policy reports nothing (the runtime sets those variables through the CSSOM), and
//   every image it re-renders has loaded (the runtime gives it the name the build copied it to);
// - a view shows a gallery and an icon only after a click: the photos load (also in the lightbox),
//   the gallery has its styles, the icon draws from the page's sprite, and no request fails;
// - its `accordion` opens and closes by a click and by Enter and Space, and slides (not with
//   reduced motion, where it opens at once);
// - its badges say the site's words ("Reception open" / "Reception closed").
//
//   node tests/web/rooms_check.mjs <chrome> <page.html>
import { spawn } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join, dirname, basename, extname } from 'node:path';

const [chrome, page] = process.argv.slice(2);
if (!chrome || !page) { console.error('usage: node rooms_check.mjs <chrome> <page.html>'); process.exit(2); }
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const root = dirname(page);
const types = { '.html': 'text/html', '.css': 'text/css', '.js': 'text/javascript', '.svg': 'image/svg+xml' };
const server = createServer((req, res) => {
  const file = join(root, decodeURIComponent(new URL(req.url, 'http://x').pathname));
  let body;
  try { body = readFileSync(file); } catch { res.writeHead(404); res.end(); return; }
  res.writeHead(200, { 'content-type': types[extname(file)] ?? 'application/octet-stream' });
  res.end(body);
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const url = `http://127.0.0.1:${server.address().port}/${basename(page)}`;

const profile = mkdtempSync(join(tmpdir(), 'nyra-rooms-'));
const proc = spawn(chrome, ['--headless=new', '--remote-debugging-port=0', `--user-data-dir=${profile}`, '--no-first-run',
  '--no-default-browser-check', '--hide-scrollbars', '--force-device-scale-factor=1',
  '--disable-background-timer-throttling', '--disable-backgrounding-occluded-windows', '--disable-renderer-backgrounding', 'about:blank'],
  { stdio: 'ignore' });
let port;
for (let i = 0; i < 200 && !port; i++) {
  await sleep(50);
  try { port = readFileSync(join(profile, 'DevToolsActivePort'), 'utf8').split('\n')[0]; } catch {}
}
if (!port) { console.error('Chrome did not start'); proc.kill(); server.close(); process.exit(2); }
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
const failures = [];
const errors = [];
listeners.add((d) => {
  if (d.method === 'Runtime.exceptionThrown') errors.push('script error: ' + (d.params.exceptionDetails.exception?.description ?? d.params.exceptionDetails.text));
  if (d.method === 'Runtime.consoleAPICalled' && d.params.type === 'error') errors.push('console error: ' + d.params.args.map((a) => a.value ?? a.description).join(' '));
  if (d.method === 'Log.entryAdded' && /Content.Security.Policy/i.test(d.params.entry.text)) errors.push('CSP: ' + d.params.entry.text.slice(0, 240));
  // (a 404 of an image the build did not write; the browser's own request for a favicon is no file of the site)
  if (d.method === 'Network.responseReceived' && d.params.response.status >= 400 && !/favicon\.ico$/.test(d.params.response.url))
    errors.push(`HTTP ${d.params.response.status}: ${d.params.response.url}`);
  if (d.method === 'Network.loadingFailed' && d.params.type === 'Image' && !d.params.canceled) errors.push(`an image failed to load: ${d.params.errorText}`);
});
await send('Page.enable');
await send('Network.enable');
await send('Page.bringToFront');
await send('Emulation.setFocusEmulationEnabled', { enabled: true });
await send('Runtime.enable');
await send('Log.enable');
await send('Emulation.setDeviceMetricsOverride', { width: 1200, height: 900, deviceScaleFactor: 1, mobile: false });
await send('Page.addScriptToEvaluateOnNewDocument', { source: `window.__csp = [];
  document.addEventListener('securitypolicyviolation', (e) => window.__csp.push(e.violatedDirective + ' ' + (e.target && e.target.outerHTML ? e.target.outerHTML.slice(0, 160) : '')));` });
const open = async (media) => {
  await send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-reduced-motion', value: media }] });
  const loaded = new Promise((r) => { const f = (d) => { if (d.method === 'Page.loadEventFired') { listeners.delete(f); r(); } }; listeners.add(f); });
  await send('Page.navigate', { url });
  await loaded;
  await sleep(300);
};
// a real click (mouse events at the element's center), as a visitor makes it
const clickAt = async (selector) => {
  // (the page scrolls smoothly: this scroll is instant, then the element is where it stays)
  const at = await run(`(async () => { const b = ${selector}; if (!b) return null; b.scrollIntoView({ block: 'center', behavior: 'instant' });
    await new Promise((r) => setTimeout(r, 100));
    const r = b.getBoundingClientRect(); return [r.left + r.width / 2, r.top + r.height / 2]; })()`);
  if (!at) return false;
  for (const type of ['mouseMoved', 'mousePressed', 'mouseReleased'])
    await send('Input.dispatchMouseEvent', { type, x: at[0], y: at[1], button: 'left', clickCount: 1 });
  return true;
};
const key = async (name) => {
  const k = name === 'Enter' ? { key: 'Enter', code: 'Enter', windowsVirtualKeyCode: 13, text: '\r' } : { key: ' ', code: 'Space', windowsVirtualKeyCode: 32, text: ' ' };
  await send('Input.dispatchKeyEvent', { type: 'keyDown', ...k });
  await send('Input.dispatchKeyEvent', { type: 'keyUp', ...k });
};

// ---- the live view: shapes after each click, no CSP report
await open('no-preference');
const SHAPES = `(() => ({
  images: [...document.querySelectorAll('[data-live] .fig-ratio img')].map((i) => { const r = i.getBoundingClientRect(); return +(r.height / r.width).toFixed(3); }),
  grids: [...document.querySelectorAll('[data-live] .grid-cols')].map((g) => getComputedStyle(g).gridTemplateColumns.split(' ').length),
}))()`;
const shapes = async (when, images, grids) => {
  const got = await run(SHAPES);
  if (got.images.length !== images) failures.push(`${when}: ${got.images.length} images, expected ${images}`);
  for (const r of got.images) if (Math.abs(r - 0.75) > 0.02) failures.push(`${when}: an image is ${r} high per width, not 4:3 (its ratio was lost)`);
  if (got.grids.length !== grids) failures.push(`${when}: ${got.grids.length} grid(8)s, expected ${grids}`);
  for (const n of got.grids) if (n !== 8) failures.push(`${when}: the grid(8) has ${n} columns`);
};
await shapes('at the start', 3, 0);
for (const [label, images, grids] of [['family', 1, 1], ['all', 3, 0], ['double', 2, 1], ['family', 1, 1], ['all', 3, 0]]) {
  if (!(await clickAt(`[...document.querySelectorAll('[data-live] button')].find((x) => x.textContent.trim() === ${JSON.stringify(label)})`)))
    failures.push(`no button "${label}"`);
  await sleep(700);
  await shapes(`after "${label}"`, images, grids);
}
// every image of the live views has loaded (scrolled to, as a lazy image loads)
const IMAGES = `(async () => {
  const out = [];
  for (const i of document.querySelectorAll('[data-live] img')) {
    i.scrollIntoView({ block: 'center', behavior: 'instant' });
    await Promise.race([i.decode().catch(() => {}), new Promise((r) => setTimeout(r, 8000))]);
    out.push([i.getAttribute('src'), i.naturalWidth]);
  }
  return out;
})()`;
const loadedImages = async (when, count) => {
  const got = await run(IMAGES);
  if (count !== undefined && got.length !== count) failures.push(`${when}: ${got.length} images in the live views, expected ${count}`);
  for (const [src, w] of got) if (!(w > 0)) failures.push(`${when}: the image "${src}" did not load`);
};
await loadedImages('after the filters', 3);

// ---- a view that shows a gallery and an icon only after a click: its images were built, the
// gallery has its styles and its lightbox, the icon is in the page's sprite
const PHOTOS = (label) => `[...document.querySelectorAll('[data-live] button')].find((x) => x.textContent.trim() === ${JSON.stringify(label)})`;
if (await run(`document.querySelectorAll('[data-live] .gallery, [data-live] .card-icon').length`)) failures.push('the gallery shows before the click');
for (let round = 1; round <= 2; round++) {
  if (!(await clickAt(PHOTOS('Show photos')))) failures.push(`round ${round}: no button "Show photos"`);
  await sleep(700);
  await loadedImages(`round ${round}, after "Show photos"`, 6);
  const look = await run(`(() => {
    const item = document.querySelector('[data-live] .gallery-item'), grid = document.querySelector('[data-live] .gallery-grid');
    const use = document.querySelector('[data-live] .card-icon svg use');
    const target = use && document.querySelector(use.getAttribute('href'));
    return { items: document.querySelectorAll('[data-live] .gallery-item').length, display: item && getComputedStyle(item).display,
      breakInside: item && getComputedStyle(item).breakInside, columns: grid && getComputedStyle(grid).columnCount,
      icon: use ? use.getAttribute('href') : null, symbol: target ? target.nodeName.toLowerCase() : null,
      iconWidth: use ? use.getBBox().width : 0 };
  })()`);
  if (look.items !== 3) failures.push(`round ${round}: ${look.items} gallery photos, expected 3`);
  if (look.display !== 'block' || look.breakInside !== 'avoid' || look.columns === 'auto')
    failures.push(`round ${round}: the gallery shown after a click is unstyled (item display ${look.display}, break-inside ${look.breakInside}, columns ${look.columns})`);
  if (look.icon !== '#i-bike' || look.symbol !== 'symbol' || !(look.iconWidth > 0))
    failures.push(`round ${round}: the icon shown after a click does not draw (${look.icon} -> ${look.symbol}, ${look.iconWidth}px wide)`);
  // the lightbox (the gallery's script) opens the photo, which loads
  await clickAt(`document.querySelector('[data-live] .gallery-item')`);
  await sleep(500);
  const box = await run(`(async () => { const d = document.querySelector('dialog.lightbox'); const i = d && d.querySelector('img');
    if (i) await Promise.race([i.decode().catch(() => {}), new Promise((r) => setTimeout(r, 8000))]);
    return { open: Boolean(d && d.open), w: i ? i.naturalWidth : 0, src: i ? i.getAttribute('src') : null }; })()`);
  if (!box.open || !(box.w > 0)) failures.push(`round ${round}: the lightbox does not show the photo (open ${box.open}, ${box.src}, ${box.w}px)`);
  if ((await run('location.href')) !== url) {
    // (without the gallery's script the click followed the photo's link)
    failures.push(`round ${round}: a click on a photo left the page`);
    await open('no-preference');
    break;
  }
  await run(`document.querySelector('dialog.lightbox')?.close()`);
  await sleep(300);
  if (!(await clickAt(PHOTOS('Hide photos')))) failures.push(`round ${round}: no button "Hide photos"`);
  await sleep(500);
  await loadedImages(`round ${round}, after "Hide photos"`, 3);
}
for (const v of await run('window.__csp')) errors.push('CSP event: ' + v);

// ---- the badges say the site's words
const badges = await run(`[...document.querySelectorAll('.badge')].map((b) => b.textContent.trim())`);
if (badges.length !== 2 || !badges.every((b) => b === 'Reception open' || b === 'Reception closed'))
  failures.push(`the badges say ${JSON.stringify(badges)}, not "Reception open" / "Reception closed"`);

// ---- the accordion: click, Enter, Space; it slides
const ITEM = (i) => `document.querySelectorAll('.acc-item')[${i}]`;
const state = (i) => run(`(() => { const d = ${ITEM(i)}; return { open: d.open, shown: d.querySelector('.acc-a').checkVisibility(), h: d.getBoundingClientRect().height }; })()`);
const items = await run(`document.querySelectorAll('.acc-item').length`);
if (items !== 3) failures.push(`${items} accordion items, expected 3`);
for (let i = 0; i < items; i++) {
  const s = await state(i);
  if (s.open || s.shown) failures.push(`question ${i + 1} is open at the start`);
}
await clickAt(`${ITEM(0)}.querySelector('summary')`);
await sleep(70);
const early = (await state(0)).h;
await sleep(900);
let s = await state(0);
if (!s.open || !s.shown) failures.push('a click does not open the first question');
if (!(early < s.h - 8)) failures.push(`the first question does not slide open (${early}px after 70 ms, ${s.h}px at the end)`);
await clickAt(`${ITEM(0)}.querySelector('summary')`);
await sleep(900);
s = await state(0);
if (s.open || s.shown) failures.push('a second click does not close the first question');
await run(`${ITEM(1)}.querySelector('summary').focus()`);
await key('Enter');
await sleep(900);
if (!(await state(1)).open) failures.push('Enter does not open the second question');
await key(' ');
await sleep(900);
if ((await state(1)).open) failures.push('Space does not close the second question');
await key(' ');
await sleep(900);
if (!(await state(1)).open) failures.push('Space does not open the second question');

// ---- with reduced motion it opens at once
await open('reduce');
await clickAt(`${ITEM(2)}.querySelector('summary')`);
await sleep(40);
const at40 = await state(2);
await sleep(600);
const done = await state(2);
if (!at40.open || Math.abs(at40.h - done.h) > 1) failures.push(`with reduced motion the third question does not open at once (${at40.h}px, then ${done.h}px)`);
for (const e of errors) failures.push(e);

ws.close();
proc.kill();
server.close();
await sleep(200);
try { rmSync(profile, { recursive: true, force: true }); } catch {}
if (failures.length) {
  console.error([...new Set(failures)].join('\n'));
  process.exit(1);
}
console.log('rooms: re-rendered images and grids keep their shape under the policy, the accordion opens and closes, the badges say the site\'s words');
