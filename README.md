# Nyra

**A programming language built for AI agents.** Compact, strict and unambiguous, with
compiler errors in JSON that an agent can act on directly. One source file compiles to
**C** (native speed) or **JavaScript** (web and Node), with more targets planned.

```
fn fib(n: int) -> int {
    if n < 2 { ret n }
    ret fib(n - 1) + fib(n - 2)
}

fn main() {
    print(fib(30))
}
```

```
$ nyra run examples/fib.nyra --time
832040
nyra 0.20 ms | cc 451.69 ms | run 140.59 ms
```

## Why

Most languages are designed for humans to read. When an AI agent writes code, other
things matter more:

- **One way to do everything.** No implicit conversions, no shadowing, no null. Fewer
  choices mean fewer wrong ones.
- **Errors are an API.** Every error has a stable code, an exact position and a fix
  hint, and `--json` makes them machine-readable.
- **The whole language fits in a prompt.** [`docs/SPEC.md`](docs/SPEC.md) is the
  complete spec, short enough to paste into an agent's context.
- **Fast.** The compiler is written in Rust with zero dependencies and compiles a
  program in well under a millisecond. The C backend gives native performance.

```
$ nyra check tests/errors/undefined_var.nyra --json
{"ok":false,"errors":[{"code":"E0201","message":"undefined variable `cout`",
  "file":"tests/errors/undefined_var.nyra","line":4,"col":11,"hint":"did you mean `count`?"}]}
```

## Quick start

You need [Rust](https://rustup.rs), plus a C compiler (gcc/clang) for the C target or
[Node.js](https://nodejs.org) for the JS target.

```
cargo build --release
./target/release/nyra run examples/hello.nyra          # native
./target/release/nyra run examples/hello.nyra --js     # JavaScript (Node)
./target/release/nyra build examples/hello.nyra        # -> hello.exe
```

| command | what it does |
|---|---|
| `nyra run <file>` | compile and run |
| `nyra build <file>` | compile to a native executable |
| `nyra check <file>` | only check for errors |

| option | meaning |
|---|---|
| `--js` | use the JavaScript backend instead of native |
| `--c` | with `build`: write the generated C instead of an executable |
| `-o <path>` | output path (`-o -` prints to stdout) |
| `--json` | errors as JSON |
| `--time` | show how long each step took |

You never pass flags to the C compiler. Nyra finds gcc/clang/tcc on its own (or uses
`NYRA_CC`) and always calls it with just `-O2 -fwrapv`: optimize, and make integer overflow
wrap the way the spec says.

## How it works

```
source → lexer → parser → type checker → backend ─┬→ C   → gcc/clang → native binary
                                                  └→ JS  → node / browser
```

| file | role |
|---|---|
| `src/lexer.rs` | text → tokens |
| `src/parser.rs` | tokens → syntax tree (recursive descent, recovers after errors) |
| `src/check.rs` | type checking, annotates the tree |
| `src/codegen/c.rs`, `js.rs` | backends |
| `src/diag.rs` | errors for humans and JSON for agents |

Every example in `examples/` runs on **every** backend in the test suite, and the output
must match exactly.

## Roadmap

- [x] v0.1: functions, `int`/`float`/`bool`/`str`, `if`/`while`/`for`, C + JS backends, JSON errors
- [ ] structs, arrays and string operations
- [ ] memory model (reference counting, no GC)
- [ ] an IR between the checker and the backends, then WASM as a third target
- [ ] browser playground
- [ ] **benchmark:** how often an LLM writes correct Nyra on the first try, and its token usage vs Python / TS / Rust
- [ ] standard library and a package/addon system

## License

MIT
