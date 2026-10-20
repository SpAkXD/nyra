"""Replace eval(): run code a model wrote in the Nyra sandbox instead of in your own process.

    python sdk/examples/python/replace_eval.py

Before:   result = eval(model_output)           # full access to your process, files, network
After:    result = box.run_json(model_output)   # no capabilities, limits on steps/memory/time
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "python" / "src"))  # not needed once installed

from nyra import NyraError, Sandbox

# code as a model would write it: compute something and print it as JSON
model_output = """
use json
struct Stats { count: int, total: int, mean: float }
let xs = [3, 1, 4, 1, 5, 9, 2, 6]
var total = 0
for x in xs {
    total += x
}
print(json.str(Stats(count: xs.len(), total: total, mean: float(total) / float(xs.len()))))
"""

box = Sandbox(fuel=1_000_000, timeout=2, memory_mb=64)
print("value:", box.run_json(model_output))

# a mistake comes back as data the model can act on, not as an exception in your process
r = box.run("let xs = [1, 2, 3]\nprint(xs[10])\n")
print("ok:", r.ok, "| error:", r.errors[0]["code"], "line", r.errors[0]["line"], "|", r.errors[0]["message"])

# and an endless loop stops at the limit
try:
    box.run_json("var i = 0\nwhile true {\n    i += 1\n}\n")
except NyraError as e:
    print("stopped by:", e.result.limit, e.result.errors[0]["code"])
