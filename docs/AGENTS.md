# AGENTS.md: working with Nyra

This project is written in Nyra (https://github.com/SpAkXD/nyra), a small, strict language for AI agents. It is not
Rust, Go, TypeScript or Python, and it is not in your training data: do not guess syntax.

## Before you write code
1. `nyra --version` must work. Websites (`use web`) need Nyra v0.8.
2. Read the agent card once per session: MCP tool `nyra_spec`, or
   https://raw.githubusercontent.com/SpAkXD/nyra/main/docs/AGENT_CARD.md (about 1,400 tokens). For a website also
   read `docs/WEB_CARD.md` (same place). `docs/SPEC.md` only when the card is not enough.
3. A feature that is not on the card does not exist. Say so and write the closest program that works.

## Edit, check, run
- After every edit: `nyra check file.nyra --json` gives `{"ok":true,...}` or `errors` with `code`, `line`, `col`,
  `hint` and sometimes `fix`. Follow the hint.
- Slips with one certain repair (`elif`, `;`, `'text'`, `xs.length()`) are repaired in memory and listed under
  `warnings`; `--fix` writes them into the file.
- Hint not enough: `nyra explain E0210` (meaning, causes, a wrong and a fixed program). Do not guess twice.
- Warnings (E0260, E0360-E0362) never fail a build but are likely bugs or slow code.
- `nyra run file.nyra` (`-- args` passes arguments). Runtime errors have a code too (exit code 101).
- Test with examples after a function: `fn sq(x: int) -> int = x * x  ex sq(3) == 9`, or
  `ex for n in 0..100: f(n) >= 0`. `nyra check` evaluates them; `nyra test file.nyra` reports each.
- State a function's rule as a contract (Nyra v0.9, docs/SPEC.md "Contracts"): lines `requires amount > 0` and
  `ensures result >= 0` after the signature. `nyra check` searches for an input that breaks it (E0421, E0422);
  every call checks it (E0420, E0421).
- Change one function of a big file by name: `nyra outline`, `nyra show file.nyra name`, `nyra edit`.

## Capabilities and the sandbox
- `json math text time random` are always allowed. `use fs`, `use input`, `use os` need
  `nyra run file.nyra --allow fs` (several: `--allow fs,os`); without it, E0290 before anything runs.
- Code nobody reviewed: add `--sandbox` (interpreter, files confined to the working folder, step, memory and
  output limits).
- Grant only what the task needs; never `--allow all` just to silence an error.

## Websites (`use web`)
- `nyra run site.nyra` serves on http://127.0.0.1:8000 and reloads on save; `nyra build site.nyra` writes
  `dist/`; `nyra check` also checks anchors, images, headings and contrast.
- Bare names: `site(...)` once, `page("/", "Title") { ... }`, then `section grid card navbar hero hours map form
  footer`. A program name equal to one is E0372.
- Look comes from the kit and theme (`site(primary:, font:, style:)`, `bg: "alt"`), motion from words
  (`anim: "rise"`, `hover: "lift"`, `scroll: "parallax"`). No class lists, inline styles or JSX; for the rest,
  `css("...")` (at the top level or as a line of a block).
- `img(src, alt)` needs `alt` (`""` = decorative); images go in `assets/`. Icons are Lucide names in text
  (`":phone: Call us"`). Unknown words are E0374 with the nearest word; an unknown icon is warning E0389 (a
  stand-in is drawn).
- State in top-level `var`s, `enum Msg`, `fn update(m: Msg)`, `button(..., on: Msg.X)`: a handler is a value.

## Mistakes models make (compile errors)
- No `;`, `elif`, `and`/`or`/`not` (`&&` `||` `!`), `i++` (`i += 1`), `null` (`int?`, `none`), classes, `impl`,
  generics, closures, `reduce` (`fold`), `switch` (`match`), `**`, `xs[-1]`.
- `{` on the same line; `//` comments; typed parameters `fn f(a: int) -> int`; `str`, not `string`.
- `let` is immutable, `var` changes; no shadowing (E0206).
- No implicit conversions: `float(n) / 2.0`, `str(n)`, `int(s)`; `"n={n}"`. Conditions are `bool`:
  `if xs.len() > 0`.
- `Point(x: 1, y: 2)`, `xs.len()` (not `len(xs)`), `use math` then `math.sqrt(x)`.
- `'a'` is a char, `"a"` a string, `s[i]` a char. `0..n` excludes `n`. `var xs: [int] = []` needs its type.
- Values are copies: change a caller's variable with `fn f(inout a: [int])`, called `f(inout xs)`.
- A program is top-level statements; no `fn main` needed.

## Save tokens
- Read the card once; look error codes up one at a time.
- Scripts, one-line functions (`fn f(x: int) -> int = x * 2`), `xs.map(x => x * 2)`, comprehensions, `print(a, b)`.
- After a failed check change only what the errors point at; `--fix` before asking a model to repair.

## Agent runtime (v0.9, docs/SERVE.md)
- `fn main() -> T`: the program prints its value as one JSON line on stdout (`print` goes to stderr);
  `nyra schema file.nyra` gives its JSON Schema. Interpreter, native and `--js` only.
- `nyra serve --mcp tools.nyra [--allow fs]`: each `pub fn` is an MCP tool (its `//` comment is the description).
- Long jobs: `nyra run job.nyra --checkpoint state.json --fuel N` pauses (exit 125) instead of failing;
  `nyra resume state.json` goes on where it stopped.
- Tests without side effects: `--mock fs=fixtures/` (writes stay in memory), `--mock input=in.txt`,
  `--mock os.env=KEY=VALUE`.

## MCP (optional)
`nyra mcp` serves the card, check, test, run, build, explain and edit as tools. Claude Code:
`claude mcp add nyra -- nyra mcp`; other clients: `{"mcpServers": {"nyra": {"command": "nyra", "args": ["mcp"]}}}`.
`nyra_run` grants only standard input unless `allow` says more; `sandbox: true` uses the interpreter.
