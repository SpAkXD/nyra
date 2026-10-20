// Replace eval(): run code a model wrote in the Nyra sandbox instead of in your own process.
//
//   node sdk/examples/js/replace_eval.mjs                    (from the repository; once installed:
//                                                             import { Sandbox } from "@nyralang/nyra")
// Before:  const result = eval(modelOutput);                 // the whole process, files, network
// After:   const result = await box.runJson(modelOutput);    // no capabilities, limits on steps/memory/time
//
// Uses the `nyra` executable when one is found (NYRA_PATH, PATH), else the bundled WebAssembly compiler.
import { NyraError, Sandbox } from "../../js/src/index.js";

const modelOutput = `
use json
let words = ["sandbox", "eval", "agent", "eval", "nyra", "agent", "eval"]
var counts: [str: int] = [:]
for w in words {
    counts[w] = counts.get(w, 0) + 1
}
print(json.str(counts))
`;

const box = new Sandbox({ fuel: 1_000_000, timeoutMs: 2000, memoryMb: 64 });
console.log(`backend: ${box.backendName}`);
console.log("value:", await box.runJson(modelOutput));

// a mistake comes back as data the model can act on
const r = await box.run("let xs = [1, 2, 3]\nprint(xs[10])\n");
console.log("ok:", r.ok, "| error:", r.errors[0].code, "line", r.errors[0].line, "|", r.errors[0].message);

// and an endless loop stops at the limit
try {
  await box.runJson("var i = 0\nwhile true {\n    i += 1\n}\n");
} catch (e) {
  if (!(e instanceof NyraError)) throw e;
  console.log("stopped by:", e.result.limit, e.result.errors[0].code);
}
