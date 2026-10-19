// node perf.js URL [RUNS=3] -> one JSON object: median FCP, LCP, CLS, load time, transferred bytes and requests.
//
// Desktop (1280x900), unthrottled (the local machine and its own network), cache disabled, a fresh browser profile
// for every run. Bytes are what went over the wire (encodedDataLength: gzip-compressed text from serve.py, and the
// remote images and fonts as their hosts send them).
const { launch, sleep } = require('./cdp');

const url = process.argv[2];
const runs = +process.argv[3] || 3;

async function once() {
  const b = await launch();
  const types = new Map(); let bytes = 0, n = 0; const byType = {}; let loaded = false;
  b.on('Network.responseReceived', (p) => types.set(p.requestId, p.type));
  b.on('Network.loadingFinished', (p) => { const t = types.get(p.requestId) || 'Other'; bytes += p.encodedDataLength; n++; byType[t] = (byType[t] || 0) + p.encodedDataLength; });
  b.on('Page.loadEventFired', () => { loaded = true; });
  await b.send('Page.enable'); await b.send('Network.enable'); await b.send('Runtime.enable');
  await b.send('Network.setCacheDisabled', { cacheDisabled: true });
  await b.send('Emulation.setDeviceMetricsOverride', { width: 1280, height: 900, deviceScaleFactor: 1, mobile: false });
  await b.send('Page.addScriptToEvaluateOnNewDocument', { source: `window.__lcp=0;new PerformanceObserver(l=>{for(const e of l.getEntries())window.__lcp=e.startTime}).observe({type:'largest-contentful-paint',buffered:true});window.__cls=0;new PerformanceObserver(l=>{for(const e of l.getEntries())if(!e.hadRecentInput)window.__cls+=e.value}).observe({type:'layout-shift',buffered:true});` });
  await b.send('Page.navigate', { url });
  for (let i = 0; i < 300 && !loaded; i++) await sleep(100);
  await sleep(3000);
  const r = await b.eval(`(()=>{const nav=performance.getEntriesByType('navigation')[0];const fcp=(performance.getEntriesByName('first-contentful-paint')[0]||{}).startTime;return {fcp,lcp:window.__lcp,cls:window.__cls,load:nav.loadEventEnd}})()`);
  await b.close();
  return { ...r, kb: bytes / 1024, n, byType };
}

(async () => {
  const all = [];
  for (let i = 0; i < runs; i++) all.push(await once());
  const med = (k) => { const v = all.map((x) => x[k] || 0).sort((a, b) => a - b); return v[Math.floor(v.length / 2)]; };
  const bt = {}; for (const [k, v] of Object.entries(all[0].byType)) bt[k] = +(v / 1024).toFixed(1);
  console.log(JSON.stringify({ url, runs, fcp_ms: Math.round(med('fcp')), lcp_ms: Math.round(med('lcp')), cls: +med('cls').toFixed(3),
    load_ms: Math.round(med('load')), kb: +med('kb').toFixed(1), requests: med('n'), kb_by_type_first_run: bt,
    lcp_all: all.map((x) => Math.round(x.lcp || 0)) }));
  process.exit(0);
})().catch((e) => { console.error(e); process.exit(1); });
