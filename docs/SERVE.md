# The agent runtime: serve, typed output, checkpoints, mocks

Four things for programs that agents write and run (since v0.9):

| | command | what it does |
|---|---|---|
| tools | `nyra serve --mcp tools.nyra` | the `pub fn`s of a file become the tools of an MCP server |
| typed output | `fn main() -> T`, `nyra schema file.nyra` | a program prints one JSON value of a known type |
| checkpoints | `nyra run --checkpoint state.json`, `nyra resume state.json` | a run that runs out of fuel pauses and goes on later |
| mocks | `nyra run --mock fs=DIR` | files, input and environment come from a mock, never the machine |

## `nyra serve --mcp`: Nyra functions as MCP tools

```nyra
// tools.nyra
struct Stats {
    count: int
    mean: float
}

// Count and mean of a list of scores.
pub fn stats(scores: [float]) -> Stats {
    var sum = 0.0
    for s in scores {
        sum += s
    }
    let n = scores.len()
    return Stats(count: n, mean: n > 0 ? sum / float(n) : 0.0)
}

// Adds two numbers; b is 1 unless given.
pub fn add(a: int, b: int = 1) -> int = a + b
```

```
nyra serve --mcp tools.nyra            # an MCP server on stdin/stdout
nyra serve --mcp --list tools.nyra     # the tools with their schemas, then exit
nyra schema --tools tools.nyra         # the same
```

- **Which functions.** Every `pub fn` of the file is a tool, under its own name; nothing else is (not `main`,
  not a function without `pub`, not the functions of the files it imports). The `//` comment lines right above a
  function are its description, followed by its Nyra signature. A tools file holds functions, structs and enums; it
  has no statements at the top level and needs no `fn main`.
- **Arguments.** `inputSchema` is an object with one property per parameter, a JSON Schema made from the
  parameter's type (see [Schemas](#schemas)); a parameter with a default value (`b: int = 1`) is optional and the
  schema shows the default. Arguments are read like `json.parse` reads: a missing argument, an unknown one or a
  value of the wrong shape is error E0345 (`argument \`a\`: expected an int`). A function with an `inout`
  parameter cannot be a tool (its arguments come from JSON): the server refuses to start and says why.
- **Results.** A function that returns a value has an `outputSchema`; the result is in `structuredContent.result`
  and, as JSON text, in the first text content. What the function printed comes back as a second text content.
- **Runs.** Every call runs in the interpreter, in the server's process, with fresh limits: `--fuel` (default
  200,000,000 steps), `--max-memory` (256M), `--max-output` (1M), `--max-depth` (20,000) and `--timeout` (10,000 ms).
  File paths are confined to the working folder. The same call with the same arguments always gives the same result
  (unless it reads a clock or a random number).
- **Capabilities.** The tools get only what `--allow` grants (`--allow fs`, `--allow fs,os`; default: nothing), or
  a mock (`--mock fs=DIR`, below). A tool whose code uses a module the server does not grant fails with E0290
  before it runs, and its description says which capabilities it needs.
- **Errors** are tool errors (`isError: true`), with the text that `nyra run --json` prints:
  `{"ok":false,"errors":[{"code":"E0240","message":"index 5 is out of bounds for length 2","file":"tools.nyra","line":12,"col":14,"hint":"..."}]}`.
  `nyra explain CODE` (or the `nyra_explain` tool of `nyra mcp`) explains each code.

### Claude Code and Claude Desktop

Claude Code:

```
claude mcp add mytools -- nyra serve --mcp /absolute/path/to/tools.nyra
claude mcp add filetools -- nyra serve --mcp /path/to/tools.nyra --allow fs
```

Claude Desktop (`claude_desktop_config.json`), Cursor and other clients:

```json
{
  "mcpServers": {
    "mytools": {
      "command": "nyra",
      "args": ["serve", "--mcp", "/absolute/path/to/tools.nyra", "--timeout", "5000"]
    }
  }
}
```

The server reads the file once, when it starts: restart it (in Claude Code `/mcp`, in Claude Desktop restart the
app) after you change the file.

## Typed output: `fn main() -> T`

```nyra
struct Item {
    name: str
    qty: int
}

fn main() -> [Item] {
    print("collecting")
    return [Item(name: "pen", qty: 2), Item(name: "ink", qty: 1)]
}
```

```
$ nyra run items.nyra 2>/dev/null
[{"name":"pen","qty":2},{"name":"ink","qty":1}]
```

- A program whose `main` returns a value prints that value as JSON on standard output: one JSON document and a line
  break, nothing else. It is written by the same code as `json.str`, so it is valid JSON of the shape of `T` by
  construction.
- What the program prints with `print` goes to **standard error** instead, so a caller reads standard output as data
  and still sees the program's messages.
- A runtime error (exit code 101) or `os.exit` prints no value: check the exit code before reading standard output.
- With statements at the top level, `main` runs after them and its value is the program's output; a call `main()`
  among them is then error E0211.
- It works in the interpreter (`--interp`, `--sandbox`, `nyra_run`), natively (C) and on the `js` target. The `py`,
  `ts`, `rs` and `go` targets refuse such a program with a clear message for now.

`nyra schema items.nyra` prints the JSON Schema of what the program prints:

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "[Item]",
  "type": "array",
  "items": {
    "title": "Item",
    "type": "object",
    "properties": {
      "name": {"type": "string"},
      "qty": {"type": "integer"}
    },
    "required": ["name", "qty"],
    "additionalProperties": false
  }
}
```

### Schemas

| Nyra type | JSON | schema |
|---|---|---|
| `int` | integer | `{"type": "integer"}` |
| `float` | number; `null` for NaN and infinities | output `{"type": ["number", "null"]}`, input `{"type": "number"}` |
| `bool`, `str` | boolean, string | `{"type": "boolean"}`, `{"type": "string"}` |
| `char` | a string of one character | `{"type": "string", "minLength": 1, "maxLength": 1}` |
| `[T]` | array | `{"type": "array", "items": T}` |
| `[str: V]` | object | `{"type": "object", "additionalProperties": V}` |
| `[K: V]` (other keys) | array of `[key, value]` pairs | an array of two-element arrays |
| `(A, B)` | array of fixed length | `prefixItems`, `minItems` = `maxItems` |
| `T?` | the value, or `null` | `{"anyOf": [T, {"type": "null"}]}` |
| struct | object with every field | `properties`, all `required`, `additionalProperties: false` |
| enum | `"Name"`, or `{"Name": [values]}` for a variant with values | `enum` of the names, or `oneOf` |

A struct or enum that contains itself (`struct Node { kids: [Node] }`) is written once under `$defs` and
referred to with `$ref`.

## Fuel and checkpoints

```
nyra run job.nyra --checkpoint state.json --fuel 50000000
nyra: paused after 50000000 steps at job.nyra:14:9 (its fuel ran out); resume with `nyra resume state.json`
nyra resume state.json                    # another 50,000,000 steps
nyra resume state.json --fuel 1000000000  # or more
```

- `--checkpoint FILE` runs the program in the interpreter. When its fuel (`--fuel N`, steps) or its time
  (`--max-time MS`) runs out, it does not fail with E0355 or E0359: it stops before the next statement, saves its
  state to FILE and exits with code **125**.
- The state is JSON: the call stack (every call that runs, its locals and where it stands: the statement, the branch,
  the part of a loop, the items a `for` has left), the strings, arrays, structs and maps the locals hold (a value two
  variables share is saved once and stays shared), the standard input that is left, the random generator, the clock of
  `time.sleep_ms`, the mocks, and the hashes of the source and of the compiled program.
- `nyra resume FILE` goes on from the statement it stopped before, with fresh fuel (`--fuel N`; default: what the run
  started with) and time (`--max-time MS`). Output that was printed and files that were written before the pause are
  not done again: the program goes on, it does not start over. A run paused and resumed prints exactly what it prints
  when it runs at once, and runs the same number of steps.
- If it pauses again, FILE is written again (`--checkpoint OTHER` writes elsewhere). When the run ends, FILE is
  removed, so the same state cannot be resumed twice by mistake.
- `nyra resume` refuses a checkpoint whose program changed (its source, a file it imports, or the version of nyra
  that compiles it): run the program again from the start.
- A program that reads standard input reads all of it when the run starts; what is left of it is in the checkpoint.
- Exit codes: 125 paused, 120 to 124 the other limits (memory, output, depth stay errors), 101 a runtime error.

## Mocks: `--mock`

```
nyra run agent.nyra --mock fs=fixtures/ --mock input=prompt.txt --mock os.env=API_KEY=test
```

| mock | the program sees |
|---|---|
| `fs=DIR` | a copy-on-write view of DIR: it reads DIR's files; `fs.write`, `fs.append`, `fs.mkdir` and `fs.remove` change a copy in memory, never DIR or anything else on the disk. Paths are relative to DIR; an absolute path or `..` is E0340 |
| `input=FILE` | the file as its standard input |
| `os.env=KEY=VALUE` | only these environment variables (repeat the flag for more) |

- A mock grants its capability (no `--allow` needed) and implies the interpreter. Like `--allow`, a run with mocks
  starts from no capability: what is not mocked or allowed is E0290. `--allow os` next to `--mock fs=DIR` gives the
  real `os` and the mocked files.
- Within a run, the program sees its own writes (`fs.read` after `fs.write` gives the new text, `fs.list` shows new
  files). `nyra serve --mcp` keeps the mock's changes for the life of the server.
- A checkpoint keeps the mock's changes, so a resumed run sees them too.

### Design note: network mocks (`--mock net=DIR`)

Nyra has no network module yet (the capability `net` exists so that one is closed by default). When `use http`
arrives, the mock follows the same rules as `fs`:

- `--mock net=fixtures/` serves every request from files: `GET https://api.example.com/users/1` reads
  `fixtures/api.example.com/users/1` (a query string becomes part of the name, `?` written as `%3F`); other methods
  read `fixtures/api.example.com/users/1.POST`. A file `NAME.status` gives the status code (default 200) and
  `NAME.headers` the headers, one `Name: value` per line.
- A request with no file is a runtime error that names the file it looked for, never a real request: a test cannot
  reach the network by accident. `--mock net=record:DIR` (with `--allow net`) would make real requests and write the
  files, to create fixtures.
- Requests made by a run are listed in the checkpoint like files written, so a resumed run does not repeat them.
