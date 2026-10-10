// The Nyra compiler as WebAssembly: a thin wrapper over its C-ABI exports (src/wasm.rs in the
// compiler). Works in a browser worker and in Node. No bindings generator, no dependencies.
//
//   const mod = await compileNyra(fetch("nyra.wasm"));   // a Response, an ArrayBuffer or bytes
//   const nyra = instantiateNyra(mod);                   // cheap: one per worker, again after a trap
//   nyra.run(src, stdin, "fuel=1000000")                 // -> {ok, exit, stdout, errors, ...}
//
// Strings go in through buffers from nyra_alloc and come back as (pointer, result_len()).

const enc = new TextEncoder();
const dec = new TextDecoder();

export async function compileNyra(source) {
  source = await source;
  if (typeof Response !== "undefined" && source instanceof Response) {
    if (!source.ok) throw new Error(`cannot load the compiler: HTTP ${source.status}`);
    if (WebAssembly.compileStreaming && (source.headers.get("content-type") || "").startsWith("application/wasm")) {
      try { return await WebAssembly.compileStreaming(source.clone()); } catch (_) { /* read the bytes below */ }
    }
    source = await source.arrayBuffer();
  }
  return WebAssembly.compile(source);
}

export function instantiateNyra(module) {
  const env = {
    nyra_now_ms: () => Date.now(),
    nyra_mono_ms: () => performance.now(),
    nyra_random: () => Math.random(),
  };
  const ex = new WebAssembly.Instance(module, { env }).exports;

  function put(text) {
    const bytes = enc.encode(text);
    const ptr = ex.nyra_alloc(bytes.length);
    new Uint8Array(ex.memory.buffer, ptr, bytes.length).set(bytes);
    return [ptr, bytes.length];
  }
  // the memory may have grown during the call: take a fresh view
  const take = (ptr) => dec.decode(new Uint8Array(ex.memory.buffer, ptr, ex.result_len()).slice());
  function call(fn, ...texts) {
    const bufs = texts.map(put);
    const out = take(fn(...bufs.flat()));
    for (const [p, n] of bufs) ex.nyra_free(p, n);
    return out;
  }
  const json = (fn, ...texts) => JSON.parse(call(fn, ...texts));

  return {
    version: () => take(ex.version()),
    check: (src) => json(ex.check, src),
    js: (src) => {
      const out = call(ex.compile_js, src);
      return out.startsWith('{"ok":false') ? JSON.parse(out) : { ok: true, js: out };
    },
    run: (src, stdin = "", limits = "") => json(ex.run_interp, src, stdin, limits),
    explain: (code) => json(ex.explain, code),
    fmt: (src) => json(ex.fmt, src),
    fix: (src) => json(ex.fix, src),
  };
}

export async function loadNyra(source) {
  return instantiateNyra(await compileNyra(source));
}
