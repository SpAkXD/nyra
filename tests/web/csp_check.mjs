// Opens every page of the built fixtures (tests/web/fixtures/*) in a headless Chrome over the DevTools
// protocol (no packages), served over http with each page's own Content-Security-Policy: it scrolls the
// page to the end (every entrance plays), clicks every event button of a live region (the regions
// render again) and fails on any CSP violation or script error. The kit, the motion script and the
// runtime set styles only through the CSSOM, never as a `style` attribute.
//
//   node tests/web/csp_check.mjs <chrome> <fixtures folder>
import { spawn } from 'node:child_process';
import { mkdtempSync, readFileSync, readdirSync, rmSync, statSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join, extname, relative, sep } from 'node:path';

const [chrome, root] = process.argv.slice(2);
if (!chrome || !root) { console.error('usage: node csp_check.mjs <chrome> <fixtures folder>'); process.exit(2); }
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const pages = [];
const walk = (d) => {
  for (const f of readdirSync(d)) {
    const p = join(d, f);
    if (statSync(p).isDirectory()) walk(p);
    else if (f.endsWith('.html')) pages.push(relative(root, p).split(sep).join('/'));
  }
};
walk(root);

const types = { '.html': 'text/html', '.css': 'text/css', '.js': 'text/javascript', '.svg': 'image/svg+xml', '.webp': 'image/webp' };
const server = createServer((req, res) => {
  const file = join(root, decodeURIComponent(new URL(req.url, 'http://x').pathname));
  let body;
  try { body = readFileSync(file); } catch { res.writeHead(404); res.end(); return; }
  res.writeHead(200, { 'content-type': types[extname(file)] ?? 'application/octet-stream' });
  res.end(body);
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const base = `http://127.0.0.1:${server.address().port}/`;

const profile = mkdtempSync(join(tmpdir(), 'nyra-csp-'));
const proc = spawn(chrome, ['--headless=new', '--remote-debugging-port=0', `--user-data-dir=${profile}`, '--no-first-run',
  '--no-default-browser-check', '--hide-scrollbars', '--force-device-scale-factor=1',
  '--disable-background-timer-throttling', '--disable-backgrounding-occluded-windows', '--disable-renderer-backgrounding', 'about:blank'], { stdio: 'ignore' });
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
let problems = [];
listeners.add((d) => {
  if (d.method === 'Runtime.exceptionThrown') problems.push('script error: ' + (d.params.exceptionDetails.exception?.description ?? d.params.exceptionDetails.text));
  if (d.method === 'Log.entryAdded' && /Content.Security.Policy/i.test(d.params.entry.text)) problems.push('CSP: ' + d.params.entry.text.slice(0, 240));
});
await send('Page.enable');
// a page in front and focused renders every frame (a CI runner's headless window may count as hidden)
await send('Page.bringToFront');
await send('Emulation.setFocusEmulationEnabled', { enabled: true });
await send('Runtime.enable');
await send('Log.enable');
await send('Emulation.setDeviceMetricsOverride', { width: 1200, height: 900, deviceScaleFactor: 1, mobile: false });
// a violation is also an event in the page: count those the log might not show
await send('Page.addScriptToEvaluateOnNewDocument', { source: `window.__csp = [];
  document.addEventListener('securitypolicyviolation', (e) => window.__csp.push(e.violatedDirective + ' ' + (e.sample || e.blockedURI)));` });

const failures = [];
for (const page of pages) {
  problems = [];
  const loaded = new Promise((r) => { const f = (d) => { if (d.method === 'Page.loadEventFired') { listeners.delete(f); r(); } }; listeners.add(f); });
  await send('Page.navigate', { url: base + page });
  await loaded;
  await run(`(async () => { for (let y = 0; y < document.body.scrollHeight; y += 400) { scrollTo(0, y); await new Promise((r) => setTimeout(r, 40)); } })()`);
  await sleep(600);
  // every event button of a live region, then the first again
  const n = await run(`document.querySelectorAll('[data-live] button[data-on]').length`);
  for (let i = 0; i <= n && n > 0; i++) {
    await run(`(() => { const b = document.querySelectorAll('[data-live] button[data-on]')[${i % Math.max(n, 1)}]; if (b) b.click(); })()`);
    await sleep(300);
  }
  await sleep(300);
  const seen = await run('window.__csp');
  for (const v of seen) problems.push('CSP event: ' + v);
  for (const p of new Set(problems)) failures.push(`${page}: ${p}`);
}

ws.close();
proc.kill();
server.close();
await sleep(200);
try { rmSync(profile, { recursive: true, force: true }); } catch {}
if (failures.length) {
  console.error(failures.join('\n'));
  process.exit(1);
}
console.log(`${pages.length} pages: no CSP violation, no script error`);
