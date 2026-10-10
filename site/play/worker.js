// A worker that owns one instance of the compiler, so the page never waits on it.
// Messages: {id, op, args} -> {id, result} | {id, crash}. The first message out is {ready}.
// A trap (a bug of the compiler, or a recursion deeper than the engine's stack) leaves the
// instance in an unknown state, so the worker makes a new one from the compiled module.
import { compileNyra, instantiateNyra } from "./nyra.js?v=play4";

const t0 = performance.now();
let mod = null, nyra = null;
const ready = compileNyra(fetch("nyra.wasm?v=play4")).then((m) => {
  mod = m;
  nyra = instantiateNyra(m);
  postMessage({ ready: true, version: nyra.version(), ms: Math.round(performance.now() - t0) });
}).catch((e) => postMessage({ ready: false, error: String(e && e.message || e) }));

const OPS = new Set(["check", "js", "run", "explain", "fmt", "fix"]);

onmessage = async ({ data }) => {
  const { id, op, args } = data;
  await ready;
  if (!nyra || !OPS.has(op)) {
    postMessage({ id, crash: nyra ? `unknown operation ${op}` : "the compiler did not load" });
    return;
  }
  try {
    postMessage({ id, result: nyra[op](...args) });
  } catch (e) {
    const msg = String(e && e.message || e);
    try { nyra = instantiateNyra(mod); } catch (_) { nyra = null; }
    postMessage({ id, crash: msg, stack: /call stack|too much recursion|stack overflow/i.test(msg) });
  }
};
