// Clicks through a live site in a headless Chrome over the DevTools protocol (no packages): the
// filter buttons of `examples/web/live.nyra` (its built fixture, tests/web/fixtures/live) must change
// which cards are shown, and every card shown must be visible, also after a re-render of the grid
// whose entrance (`anim: "stagger"`) has played.
//
//   node tests/web/live_check.mjs <chrome> <page.html>
import { spawn } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join, dirname, basename, extname } from 'node:path';

const [chrome, page] = process.argv.slice(2);
if (!chrome || !page) { console.error('usage: node live_check.mjs <chrome> <page.html>'); process.exit(2); }
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// the site's folder over http, as it is served (its policy allows scripts from 'self' only)
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

const profile = mkdtempSync(join(tmpdir(), 'nyra-live-'));
const proc = spawn(chrome, ['--headless=new', '--remote-debugging-port=0', `--user-data-dir=${profile}`, '--no-first-run',
  '--no-default-browser-check', '--hide-scrollbars', '--allow-file-access-from-files', '--force-device-scale-factor=1', 'about:blank'],
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
const errors = [];
listeners.add((d) => {
  if (d.method === 'Runtime.exceptionThrown') errors.push(d.params.exceptionDetails.exception?.description ?? d.params.exceptionDetails.text);
  if (d.method === 'Runtime.consoleAPICalled' && d.params.type === 'error') errors.push(d.params.args.map((a) => a.value ?? a.description).join(' '));
});
await send('Page.enable');
await send('Runtime.enable');
await send('Emulation.setDeviceMetricsOverride', { width: 1200, height: 900, deviceScaleFactor: 1, mobile: false });
const loaded = new Promise((r) => { const f = (d) => { if (d.method === 'Page.loadEventFired') { listeners.delete(f); r(); } }; listeners.add(f); });
await send('Page.navigate', { url });
await loaded;

const failures = [];
// the cards of the live region: their titles, and the ones not fully visible
const CARDS = `(() => {
  const cards = [...document.querySelectorAll('[data-live] .card')];
  return { titles: cards.map((c) => c.querySelector('.card-title').textContent),
    hidden: cards.filter((c) => +getComputedStyle(c).opacity < .99).map((c) => c.querySelector('.card-title').textContent) };
})()`;
// a real click (mouse events at the button's center), as a visitor makes it
const click = async (label) => {
  const at = await run(`(() => { const b = [...document.querySelectorAll('[data-live] button')].find((x) => x.textContent.trim() === ${JSON.stringify(label)});
    if (!b) return null; b.scrollIntoView({ block: 'center' }); const r = b.getBoundingClientRect(); return [r.left + r.width / 2, r.top + r.height / 2]; })()`);
  if (!at) { failures.push(`no button "${label}"`); return; }
  for (const type of ['mouseMoved', 'mousePressed', 'mouseReleased'])
    await send('Input.dispatchMouseEvent', { type, x: at[0], y: at[1], button: 'left', clickCount: 1 });
  await sleep(1500);
};
const expect = async (when, titles) => {
  const got = await run(CARDS);
  if (JSON.stringify(got.titles) !== JSON.stringify(titles)) failures.push(`${when}: cards ${JSON.stringify(got.titles)}, expected ${JSON.stringify(titles)}`);
  if (got.hidden.length) failures.push(`${when}: cards not visible: ${JSON.stringify(got.hidden)}`);
};

// the grid scrolls into view and its entrance plays
await run(`document.querySelector('[data-live] .grid, [data-live] .card').scrollIntoView({ block: 'center' })`);
await sleep(3000);
const all = (await run(CARDS)).titles;
if (all.length < 2) failures.push(`only ${all.length} cards at the start`);
await expect('at the start', all);
// every button of the live region, in turn, then the first again: each shows its cards, all visible,
// and the buttons show at least two different sets
const labels = await run(`[...document.querySelectorAll('[data-live] button[data-on]')].map((b) => b.textContent.trim())`);
const seen = new Set();
for (const label of [...labels, labels[0]]) {
  await click(label);
  const got = await run(CARDS);
  if (got.hidden.length) failures.push(`after "${label}": cards not visible: ${JSON.stringify(got.hidden)}`);
  seen.add(JSON.stringify(got.titles));
}
if (seen.size < 2) failures.push(`the buttons ${JSON.stringify(labels)} change nothing: ${[...seen].join(' ')}`);
// examples/web/live.nyra: the filter shows exactly the cards of its kind
if (labels.includes('sladké')) {
  await click('sladké');
  await expect('after "sladké"', ['Croissant']);
  await click('vše');
  await expect('after "vše"', all);
}
for (const e of errors) failures.push(`script error: ${e}`);

ws.close();
proc.kill();
server.close();
await sleep(200);
try { rmSync(profile, { recursive: true, force: true }); } catch {}
if (failures.length) {
  console.error(failures.join('\n'));
  process.exit(1);
}
console.log('live site: the filter works, every card shown is visible');
