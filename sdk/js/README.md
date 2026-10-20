# @nyralang/nyra

Run code that an AI wrote, safely. [Nyra](https://nyralang.dev) is a small language made for AI agents;
this package runs it in a sandbox: no files, OS or network unless you grant them, and limits on steps,
memory, output, call depth and time. Errors come back as data (code, line, column, hint, fix), so a model
can repair its program and try again.

No runtime dependencies. ESM with TypeScript types. Two backends behind one API:

- **native**: the `nyra` executable (`NYRA_PATH` or `PATH`) in a child process, always with `--sandbox`.
  Node.js, Deno, Bun.
- **wasm**: the whole compiler and interpreter as WebAssembly, bundled (`wasm/nyra.wasm`, about 1.8 MB,
  620 KB gzipped). No executable needed: Node.js, Deno, Bun, browsers, Cloudflare Workers. It grants only
  `input`.

`backend: "auto"` (the default) uses the executable when one is found, else WebAssembly.

```js
import { Sandbox } from "@nyralang/nyra";

const box = new Sandbox({ fuel: 10_000_000, timeoutMs: 5000, memoryMb: 128 });
const r = await box.run('use input\nprint("hi {input.line()}")\n', "Ada\n");
r.ok, r.stdout;                 // true, "hi Ada\n"

const bad = await box.run("let xs = [1, 2]\nprint(xs[5])\n");
bad.errors[0];                  // { code: "E0240", message: "index 5 is out of bounds for length 2", line: 2, ... }
bad.feedback();                 // the text to give back to the model

await box.runJson('use json\nprint(json.str([1, 2, 3]))\n');   // [1, 2, 3]
await box.check(code);          // { ok, errors, warnings }
await box.fix(code);            // the code with its certain fixes applied
await box.explain("E0240");     // what an error code means
```

Options: `fuel`, `timeoutMs`, `memoryMb`, `maxOutput`, `maxDepth`, `allow` (native), `cwd` (native),
`strict`, `native` (compile natively, **no sandbox**), `backend`, `binary`, `wasm`.

## Tools for model APIs

```js
import { anthropicToolSpec, openaiToolSpec, runTool } from "@nyralang/nyra";
const tools = [anthropicToolSpec()];                 // { name: "run_nyra", description, input_schema }
const text = await runTool(toolUse.input, box);      // the tool_result content
```

## Where it runs

- **Bundlers** (Vite, webpack, esbuild): `package.json` `imports` pick the browser files, and
  `new URL("../wasm/nyra.wasm", import.meta.url)` makes nyra.wasm an asset.
- **Browsers without a bundler**: add an import map, see `test/browser.html`.
- **Cloudflare Workers** cannot compile WebAssembly from bytes at run time; import the module:

  ```js
  import wasm from "@nyralang/nyra/nyra.wasm";
  const box = new Sandbox({ wasm });
  ```

  The time limit needs a clock that moves during a call, which Workers do not have: `fuel` still stops a
  run.
- In a browser a run blocks the thread it is on: use a Web Worker for long programs.

## Building nyra.wasm

`wasm/nyra.wasm` is not in git; it is built from the compiler's sources (like the playground's):

```
npm run build:wasm       # python tools/build_wasm.py --no-copy, then copy it here
npm run copy:wasm        # only copy target/wasm32-unknown-unknown/wasm/nyra.wasm
npm test                 # node --test (native: target/release/nyra or NYRA_PATH)
```

Full reference: `docs/SDK.md` in the repository.
