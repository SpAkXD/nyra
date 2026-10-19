//! `nyra new <kind> <name>`: starts a project from a template. Each kind is a folder with a program
//! that compiles and passes its examples, a README.md with the commands to run it, a `.gitignore`
//! for what `nyra build` writes, and an AGENTS.md (docs/AGENTS.md) for the AI agent that will work on it.
//! `tests/new.rs` builds and runs every kind and every command in its README.

use std::path::Path;
use std::process::ExitCode;

/// The guide for AI agents, copied into every project.
const AGENTS: &str = include_str!("../docs/AGENTS.md");

/// What `nyra build` writes into a project (the compiled-program cache is in the system's temp folder).
const GITIGNORE: &str = "\
# what `nyra build` writes
/main
/main.exe
/main.c
/main.js
dist/
";

pub struct Kind {
    pub name: &'static str,
    pub about: &'static str,
    /// The files besides README.md, .gitignore and AGENTS.md. `__NAME__` is replaced by the project's name.
    pub files: &'static [(&'static str, &'static str)],
    pub readme: &'static str,
}

pub const KINDS: &[Kind] = &[
    Kind {
        name: "script",
        about: "a program with functions and their tests (`ex`)",
        files: &[("main.nyra", SCRIPT)],
        readme: SCRIPT_README,
    },
    Kind {
        name: "cli",
        about: "a command-line tool: arguments, standard input, tests",
        files: &[("main.nyra", CLI)],
        readme: CLI_README,
    },
    Kind {
        name: "site",
        about: "a one-page website (`use web`) that builds to dist/",
        files: &[("main.nyra", SITE)],
        readme: SITE_README,
    },
    Kind {
        name: "lib",
        about: "a module of `pub` functions and a program that uses it",
        files: &[("lib.nyra", LIB), ("main.nyra", LIB_MAIN)],
        readme: LIB_README,
    },
];

fn usage() -> String {
    let mut s = String::from("usage: nyra new <kind> <name>    start a project in the folder <name>\n\nkinds:\n");
    for k in KINDS {
        s += &format!("  {:<8}{}\n", k.name, k.about);
    }
    s += "\nexample: nyra new script hello && cd hello && nyra run main.nyra\n";
    s
}

pub fn run(args: Vec<String>) -> ExitCode {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        [] | ["-h" | "--help" | "help"] => {
            print!("{}", usage());
            ExitCode::SUCCESS
        }
        [kind] => fail(&format!("`nyra new {kind}` needs a name for the project's folder: `nyra new {kind} hello`")),
        [kind, name] => match create(kind, name) {
            Ok(text) => {
                print!("{text}");
                ExitCode::SUCCESS
            }
            Err(e) => fail(&e),
        },
        _ => fail(&format!("too many arguments\n\n{}", usage())),
    }
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("nyra new: {msg}");
    ExitCode::from(2)
}

/// Writes the project `kind` into the folder `dir` and returns what to tell the user.
pub fn create(kind: &str, dir: &str) -> Result<String, String> {
    let Some(k) = KINDS.iter().find(|k| k.name == kind) else {
        let names: Vec<&str> = KINDS.iter().map(|k| k.name).collect();
        return Err(format!("unknown kind `{kind}`: use one of {}", names.join(", ")));
    };
    let path = Path::new(dir);
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.') {
        return Err(format!("`{dir}` is not a good folder name: use letters, digits, `-` and `_` (like `my-app`)"));
    }
    if path.exists() {
        let empty = std::fs::read_dir(path).map(|mut d| d.next().is_none()).unwrap_or(false);
        if !empty {
            return Err(format!("`{dir}` already exists and is not empty: pick another name"));
        }
    }
    let title = title(name);
    let fill = |t: &str| t.replace("__NAME__", name).replace("__TITLE__", &title);
    let mut files: Vec<(&str, String)> = k.files.iter().map(|(f, t)| (*f, fill(t))).collect();
    files.push(("README.md", fill(k.readme)));
    files.push((".gitignore", GITIGNORE.to_string()));
    files.push(("AGENTS.md", AGENTS.to_string()));
    std::fs::create_dir_all(path).map_err(|e| format!("cannot create `{dir}`: {e}"))?;
    let mut out = format!("Created {} ({}):\n", dir, k.about);
    for (file, text) in &files {
        std::fs::write(path.join(file), text).map_err(|e| format!("cannot write `{}`: {e}", path.join(file).display()))?;
        out += &format!("  {}/{file}\n", dir.trim_end_matches(['/', '\\']));
    }
    let first =
        if k.name == "site" { "nyra run main.nyra      # http://127.0.0.1:8000, reloads when you save" } else { "nyra run main.nyra" };
    out += &format!("\nNext:\n  cd {dir}\n  {first}\n  nyra test main.nyra\n");
    Ok(out)
}

/// `my-app` is "My App".
fn title(name: &str) -> String {
    name.split(['-', '_', '.'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

const SCRIPT: &str = r##"// __NAME__: a Nyra script. Each `ex` line is a test: `nyra check` and `nyra test` run them.

fn greet(name: str) -> str = "Hello, {name}!"
ex greet("Nyra") == "Hello, Nyra!"

fn squares(n: int) -> [int] = [i * i for i in 1..n + 1]
ex squares(3) == [1, 4, 9], squares(0) == []

fn mean(xs: [int]) -> float = xs.len() == 0 ? 0.0 : float(xs.sum()) / float(xs.len())
ex mean([1, 2, 3, 4]) == 2.5, mean([]) == 0.0

let nums = squares(5)
print(greet("world"))
print("squares:", nums)
print("mean: {mean(nums):.1}")
"##;

const SCRIPT_README: &str = r##"# __TITLE__

A Nyra script: a few functions, their tests (the `ex` lines), and the lines that use them.

## Commands

```sh
nyra run main.nyra
nyra test main.nyra
nyra check main.nyra --json
nyra build main.nyra
```

- `nyra run` runs it (in the interpreter at once, natively if it runs long).
- `nyra test` runs every `ex` line and reports the ones that fail.
- `nyra check --json` reports errors as JSON, for AI agents.
- `nyra build` writes a native executable, `main` (`main.exe` on Windows); this needs a C compiler.

## Next

Add a function and an `ex` line under it, then `nyra test main.nyra`.
The language: https://nyralang.dev and `docs/AGENT_CARD.md` in the Nyra repository.
An AI agent working here should read AGENTS.md first.
"##;

const CLI: &str = r##"// __NAME__: a command-line tool that counts words, in its arguments or in standard input.
use os
use input
use text

struct Options { top: int, words: [str], help: bool }

fn parse_args(args: [str]) -> Options {
    var o = Options(top: 0, words: [], help: false)
    var i = 0
    while i < args.len() {
        let a = args[i]
        if a == "-h" || a == "--help" {
            o.help = true
        } else if a == "--top" && i + 1 < args.len() && text.is_int(args[i + 1]) {
            o.top = int(args[i + 1])
            i += 1
        } else {
            o.words.push(a.lower())
        }
        i += 1
    }
    return o
}
ex parse_args(["--top", "2", "A", "b"]).top == 2, parse_args(["A", "b"]).words == ["a", "b"]

fn words_in(line: str) -> [str] = [w.lower() for w in line.split(" ") if w.len() > 0]
ex words_in("  The cat  sat ") == ["the", "cat", "sat"]

fn count(words: [str]) -> [(str, int)] {
    var seen: [str: int] = [:]
    for w in words {
        seen[w] = seen.get(w, 0) + 1
    }
    return seen.items().sorted_by(p => -p.1)
}
ex count(["b", "a", "b"]) == [("b", 2), ("a", 1)]

let o = parse_args(os.args())
if o.help {
    print("usage: __NAME__ [--top N] [words...]")
    print("counts the words given as arguments, or the words of standard input when there are none")
    print("  --top N   show only the N most frequent words")
    os.exit(0)
}
var words = o.words
if words.len() == 0 {
    for line in input.lines() {
        words += words_in(line)
    }
}
let counts = count(words)
let shown = o.top > 0 && o.top < counts.len() ? counts[0..o.top] : counts
for (w, n) in shown {
    print("{n:>5}  {w}")
}
print("{words.len()} words, {counts.len()} different")
"##;

const CLI_README: &str = r##"# __TITLE__

A command-line tool written in Nyra: it counts words, from its arguments or from standard input.

## Commands

```sh
nyra run main.nyra -- the cat and the hat
nyra run main.nyra -- --top 1 a b a
echo "one two two" | nyra run main.nyra
nyra test main.nyra
nyra build main.nyra
```

- Arguments after `--` go to the program (`os.args()`); with none it reads standard input (`input.lines()`).
- `nyra test` runs the `ex` lines, the tests of each function.
- `nyra build` writes a native executable, `main` (`main.exe` on Windows); this needs a C compiler. Then
  `./main --top 3 < notes.txt`.

## Next

Add an option in `parse_args` and an `ex` line that checks it, then `nyra test main.nyra`.
The language: https://nyralang.dev. An AI agent working here should read AGENTS.md first.
"##;

const SITE: &str = r##"// __NAME__: a one-page website. `nyra run main.nyra` serves it and reloads when you save;
// `nyra build main.nyra` writes the finished site to dist/.
use web

site("__TITLE__", desc: "A small, fast website made with Nyra.", primary: "#2F4B7C", accent: "#E07A1F",
    display: "Fraunces", style: "soft", lang: "en")

struct Service { name: str, kind: str, text: str, icon: str }

let services = [
    Service(name: "Websites", kind: "web", text: "Fast pages that are easy to change.", icon: "monitor"),
    Service(name: "Online shops", kind: "web", text: "A small catalog with a simple order form.", icon: "shopping-cart"),
    Service(name: "Logos", kind: "brand", text: "A mark and colors that fit you.", icon: "palette"),
    Service(name: "Print", kind: "brand", text: "Business cards, flyers and posters.", icon: "printer")
]
let kinds = [("all", "All"), ("web", "Web"), ("brand", "Brand")]
let links = [("Services", "#services"), ("Hours", "#hours"), ("Contact", "#contact")]
let opening = [("Monday - Friday", "1-5 9:00-17:00"), ("Saturday", "6 10:00-13:00"), ("Sunday", "")]

// The filter buttons change `shown`; `services_grid()` reads it, so it updates in the browser.
enum Msg { Show(str) }
var shown = "all"
fn update(m: Msg) {
    match m {
        Msg.Show(k) => shown = k
    }
}

fn services_grid() -> Node = stack {
    row {
        for k in kinds {
            button(k.1, on: Msg.Show(k.0), active: k.0 == shown)
        }
    }
    grid(2, anim: "stagger", hover: "lift") {
        for s in services.filter(x => shown == "all" || x.kind == shown) {
            card(s.name, s.text, icon: s.icon)
        }
    }
}

page("/", "__TITLE__", desc: "Websites, logos and print for small businesses.") {
    navbar("__TITLE__", links, cta: ("Write to us", "#contact"))
    hero("Clear design for small businesses", kicker: "__TITLE__", anim: "rise",
        text: "We make websites, logos and print that are simple to use and easy to change.",
        buttons: [("Our services", "#services"), ("Contact", "#contact")], badge: opening)
    section("services", "What we do", kicker: "Services", text: "Pick a kind to filter the list.", anim: "rise") {
        services_grid()
    }
    section("hours", "Opening hours", kicker: "Visit us", bg: "alt", anim: "rise") {
        grid(2, widths: "3fr 2fr") {
            hours(opening)
            card("Good to know", "Call ahead and we will have coffee ready.", icon: "coffee")
        }
    }
    section("contact", "Write to us", kicker: "Contact", width: "narrow", text: "We answer within a day.") {
        form("Send a message", mailto: "hello@example.com") {
            field("name", "Your name", req: true)
            field("email", "E-mail", type: "email", req: true)
            field("message", "Message", type: "area", req: true)
        }
    }
    footer(text: "__TITLE__ - made with [Nyra](https://nyralang.dev).", links: links, small: ["Copyright 2026 __TITLE__"])
}
"##;

const SITE_README: &str = r##"# __TITLE__

A one-page website written in Nyra (`use web`).

## Commands

```sh
nyra run main.nyra
nyra check main.nyra
nyra build main.nyra
```

- `nyra run` serves the site on http://127.0.0.1:8000 and reloads the page when you save (`--port N` for
  another port).
- `nyra check` also checks the links, images, headings and color contrast.
- `nyra build` writes the finished site to `dist/`: copy that folder to any web host.

## Next

Change the texts and colors in `main.nyra`, add a `section`, or put images in an `assets/` folder and
show them with `img("photo.jpg", "what it shows")`. The filter buttons show how a page reacts to clicks:
state in a `var`, an `enum Msg`, and `fn update`. The building blocks: `docs/WEB_CARD.md` (short) and
`docs/WEB.md` in the Nyra repository.
An AI agent working here should read AGENTS.md first.
"##;

const LIB: &str = r##"// The library of __NAME__: what is `pub` can be used by a file that says `use ./lib`.
// Everything else (here `spread`) stays private to this file.

pub struct Summary { count: int, low: int, high: int, mean: float }

pub fn summarize(xs: [int]) -> Summary {
    if xs.len() == 0 {
        return Summary(count: 0, low: 0, high: 0, mean: 0.0)
    }
    return Summary(count: xs.len(), low: xs.min(), high: xs.max(), mean: float(xs.sum()) / float(xs.len()))
}
ex summarize([3, 1, 2]).mean == 2.0, summarize([]).count == 0

pub fn describe(s: Summary) -> str = "{s.count} values from {s.low} to {s.high}, mean {s.mean:.2}, spread {spread(s)}"

fn spread(s: Summary) -> int = s.high - s.low
ex spread(Summary(count: 2, low: 4, high: 9, mean: 6.5)) == 5
"##;

const LIB_MAIN: &str = r##"// __NAME__: a program that uses the module in lib.nyra.
use ./lib

let temps = [12, 15, 9, 18, 21, 17, 11]
let s = lib.summarize(temps)
print(lib.describe(s))
if s.mean > 14.0 {
    print("a warm week")
}
ex lib.summarize([5]).high == 5
"##;

const LIB_README: &str = r##"# __TITLE__

A Nyra module (`lib.nyra`, its `pub` functions and struct) and a program that uses it (`main.nyra`).

## Commands

```sh
nyra run main.nyra
nyra test main.nyra
nyra build main.nyra
```

- `use ./lib` in `main.nyra` imports `lib.nyra` from the same folder; its functions are called as `lib.name(...)`.
- Only what is marked `pub` can be used from another file.
- `nyra test main.nyra` runs the `ex` lines of `main.nyra` and of the files it imports. (`lib.nyra` has no
  statements of its own, so it is checked and tested through a program that uses it.)

## Next

Add a `pub fn` to `lib.nyra` with an `ex` line, use it in `main.nyra`, then `nyra test main.nyra`.
The language: https://nyralang.dev. An AI agent working here should read AGENTS.md first.
"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles() {
        assert_eq!(title("my-app"), "My App");
        assert_eq!(title("hello"), "Hello");
        assert_eq!(title("a_b.c"), "A B C");
    }

    #[test]
    fn every_kind_has_main_and_a_readme_with_commands() {
        for k in KINDS {
            assert!(k.files.iter().any(|(f, _)| *f == "main.nyra"), "{}", k.name);
            assert!(k.readme.contains("```sh\nnyra "), "{}", k.name);
        }
    }
}
