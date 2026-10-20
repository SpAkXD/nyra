# nyra (Python)

Run code that an AI wrote, safely. [Nyra](https://nyralang.dev) is a small language made for AI agents;
this package runs it in the `nyra` sandbox: no files, OS or network unless you grant them, and limits on
steps, memory, output, call depth and time. Errors come back as data (code, line, column, hint, fix), so a
model can repair its program and try again.

Pure Python, no dependencies, Python 3.9+.

```python
from nyra import Sandbox

box = Sandbox(fuel=10_000_000, timeout=5, memory_mb=128)
r = box.run('use input\nprint("hi {input.line()}")\n', stdin="Ada\n")
r.ok, r.stdout          # True, 'hi Ada\n'

r = box.run("let xs = [1, 2]\nprint(xs[5])\n")
r.errors[0]             # {'code': 'E0240', 'message': 'index 5 is out of bounds for length 2', 'line': 2, ...}
r.feedback()            # the text to give back to the model

box.run_json('use json\nprint(json.str([1, 2, 3]))\n')   # [1, 2, 3]
box.check(code)         # CheckResult(ok, errors, warnings)
box.fix(code)           # the code with its certain fixes applied
box.explain("E0240")    # what an error code means
```

## The binary

The package drives the `nyra` executable. It looks in `NYRA_PATH` (the file or its folder), then `PATH`,
then the cache of `nyra.install()`. Nothing is downloaded unless you call:

```python
import nyra
nyra.install()            # the latest release for this computer, SHA-256 checked, into the user cache
nyra.install("0.9.0")     # a given version
```

## Sandbox options

| option | default | |
|---|---|---|
| `fuel` | 100,000,000 | interpreter steps (E0355, exit 120) |
| `timeout` | 10 | seconds of real time (E0359, exit 124) |
| `memory_mb` | 256 | heap (E0356, exit 121) |
| `max_output` | 1 MiB | bytes printed (E0357, exit 122) |
| `max_depth` | interpreter's | nested calls (E0358, exit 123) |
| `allow` | `[]` | capabilities besides `input`: `"fs"`, `"os"`, `"net"`, `"all"` |
| `cwd` | fresh temp folder | the working folder; with `allow=["fs"]` files stay below it |
| `strict` | `False` | do not repair errors with one certain fix in memory |
| `native` | `False` | compile and run natively: **no sandbox**, trusted code only |

`run(code, stdin="", args=None, files=None)`: `args` go to `os.args()` (needs `allow=["os"]`), `files` are
more modules next to the program (`use ./shapes`). Every call uses its own temporary folder, removed after.

## Tools for agent frameworks

```python
from nyra.tools import anthropic_tool_spec, openai_tool_spec, run_tool
from nyra.tools import langchain_tool, llamaindex_tool, crewai_tool, autogen_function
```

The framework helpers return the framework's tool object when it is installed (imported only then), else a
plain callable `NyraTool` with `name`, `description` and a JSON schema in `parameters`. Examples:
`sdk/examples/python/` in the repository. Full reference: `docs/SDK.md`.

## Tests

```
cargo build --release          # from the repository root: target/release/nyra
python -m unittest discover -s sdk/python/tests      # or: python -m pytest sdk/python/tests
```
