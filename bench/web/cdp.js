// Minimal Chrome DevTools Protocol client for headless Brave (or Chrome): no npm packages, Node 22+ only
// (global fetch and WebSocket). Used by check.js and perf.js.
//
//   const { launch } = require('./cdp');
//   const b = await launch();            // fresh profile, headless
//   const page = await b.page();         // { send(method, params), on(method, fn), eval(expr), close() }
//   ...
//   await b.close();                     // kills the browser and deletes the profile
//
// BROWSER (env) overrides the executable; the default is Brave on Windows.
const { spawn } = require('child_process');
const fs = require('fs');
const os = require('os');
const path = require('path');

const BROWSER = process.env.BROWSER || 'C:/Program Files/BraveSoftware/Brave-Browser/Application/brave.exe';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function launch() {
  const port = 9300 + Math.floor(Math.random() * 600);
  const prof = fs.mkdtempSync(path.join(os.tmpdir(), 'nyra-webbench-'));
  const proc = spawn(BROWSER, ['--headless=new', '--disable-gpu', '--hide-scrollbars', '--mute-audio',
    '--no-first-run', '--no-default-browser-check', '--disable-extensions', '--disable-background-networking',
    '--disable-component-update', '--disable-sync', '--force-color-profile=srgb',
    '--disable-site-isolation-trials', '--disable-features=IsolateOrigins,site-per-process',
    '--remote-debugging-port=' + port, '--user-data-dir=' + prof, 'about:blank'], { stdio: 'ignore' });
  let targets = null;
  for (let i = 0; i < 100; i++) {
    try { targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json(); if (targets.some((t) => t.type === 'page')) break; } catch (e) { }
    await sleep(150);
  }
  if (!targets) { proc.kill(); throw new Error('browser did not start'); }
  const t = targets.find((x) => x.type === 'page');
  const ws = new WebSocket(t.webSocketDebuggerUrl);
  await new Promise((r, j) => { ws.onopen = r; ws.onerror = j; });
  let id = 0;
  const pending = new Map();
  const listeners = new Map();
  ws.onmessage = (m) => {
    const d = JSON.parse(m.data);
    if (d.id && pending.has(d.id)) { pending.get(d.id)(d); pending.delete(d.id); return; }
    for (const fn of listeners.get(d.method) || []) { try { fn(d.params); } catch (e) { } }
  };
  const send = (method, params = {}, timeout = 30000) => new Promise((resolve, reject) => {
    const i = ++id;
    const timer = setTimeout(() => { pending.delete(i); reject(new Error('CDP timeout: ' + method)); }, timeout);
    pending.set(i, (d) => { clearTimeout(timer); d.error ? reject(new Error(method + ': ' + d.error.message)) : resolve(d.result); });
    ws.send(JSON.stringify({ id: i, method, params }));
  });
  const on = (method, fn) => { if (!listeners.has(method)) listeners.set(method, []); listeners.get(method).push(fn); };
  const evaluate = async (expression, timeout = 30000) => {
    const r = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true }, timeout);
    if (r.exceptionDetails) throw new Error('page eval: ' + (r.exceptionDetails.exception?.description || r.exceptionDetails.text).split('\n')[0]);
    return r.result.value;
  };
  return {
    send, on, eval: evaluate, sleep,
    async close() {
      try { ws.close(); } catch (e) { }
      proc.kill();
      await sleep(500);
      for (let i = 0; i < 5; i++) { try { fs.rmSync(prof, { recursive: true, force: true }); break; } catch (e) { await sleep(400); } }
    },
  };
}

module.exports = { launch, sleep, BROWSER };
