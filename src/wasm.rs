//! The compiler as WebAssembly, for the playground on nyralang.dev: plain C-ABI functions, no
//! bindings generator, built by `tools/build_wasm.py`.
//!
//! Strings travel as UTF-8 through linear memory:
//!
//! 1. the page reserves a buffer with `nyra_alloc(len)`, writes the bytes there and passes
//!    `(ptr, len)`; it frees the buffer with `nyra_free(ptr, len)` when the call returns;
//! 2. every function returns a pointer to its result, and `result_len()` is the result's length
//!    in bytes. The result belongs to the module and stays valid until the next call.
//!
//! | export | result |
//! |---|---|
//! | `check(src)` | `{"ok","errors":[..],"warnings"?:[..]}`, as `nyra check --json --strict` |
//! | `compile_js(src)` | the JavaScript program, or `{"ok":false,"errors":[..]}` (starts with `{"ok":false`) |
//! | `run_interp(src, stdin, limits)` | `{"ok","exit","stdout","errors"?,"warnings"?,"fixed"?,"steps","ms"}` |
//! | `explain(code)` | the entry of `nyra explain CODE --json` plus `"text"`; `""`: the list of codes |
//! | `fmt(src)` | `{"ok":true,"source","changed","fixed"}`, or the errors |
//! | `fix(src)` | `{"ok","source","fixed","errors"}`: the certain fixes applied |
//! | `version()` | `0.6.0` |
//!
//! The playground grants only the capability `input` (the stdin box): a `use fs` or `use os` is
//! error E0290, as it is under `nyra run --sandbox --allow input`. Programs run in the sandboxed
//! interpreter (`src/sandbox.rs`) with the limits in `limits`: `fuel=N memory=SIZE output=SIZE
//! depth=N time=MS strict=1`, any of them, separated by spaces or commas.
//!
//! The functions are ordinary Rust as well (`check_json` and the others), so the native unit
//! tests below call them; the C-ABI wrappers are only exported from the WebAssembly build.

use std::cell::RefCell;

use crate::clock::Instant;
use crate::diag::{self, json_str, Diag};
use crate::{ast, caps, explain, fix, fmt, sandbox};

/// The name of the program in messages.
const FILE: &str = "main.nyra";

/// Limits of a run in the browser (each can be changed with the `limits` argument).
pub const FUEL: u64 = 100_000_000;
pub const MEMORY: u64 = 256 << 20;
pub const OUTPUT: u64 = 1 << 20;
/// An interpreted call takes several WebAssembly frames on the engine's own stack, which is small in
/// a browser worker (Chrome overflows between 800 and 1,000 calls of a simple recursive function).
/// Deeper recursion that still overflows it traps; the page reports that as E0358 too.
pub const DEPTH: usize = 500;
pub const TIME_MS: u64 = 5_000;

/// What the playground grants: standard input only.
fn grant() -> caps::Grant {
    let mut g = caps::Grant::none();
    g.allow("input").expect("`input` is a capability");
    g
}

/// The hint of E0290 in the playground (a sentence that starts with a verb).
fn flag(cap: &str) -> String {
    format!("run it on your own machine with `nyra run main.nyra --allow {cap}` (the playground grants only `input`)")
}

fn compile_here(src: &str) -> Result<ast::Program, Vec<Diag>> {
    crate::compile_granted(src, &grant(), &flag)
}

fn warnings_field() -> String {
    diag::warnings_json(FILE)
}

/// `check`: errors and warnings, nothing repaired.
pub fn check_json(src: &str) -> String {
    crate::modules::set_main(None);
    match compile_here(src) {
        Ok(prog) => {
            crate::perfwarn::check(&prog, false);
            diag::render_json(&[], FILE)
        }
        Err(diags) => diag::render_json(&diags, FILE),
    }
}

/// `compile_js`: like `nyra build --js -o -` (errors with one certain fix are repaired in memory).
pub fn compile_js_text(src: &str) -> String {
    crate::modules::set_main(None);
    let compile = |s: &str| crate::compile_granted(s, &caps::Grant::all(), &flag);
    let prog = match compile(src) {
        Ok(p) => p,
        Err(diags) => match fix::repair(src, diags.clone(), compile) {
            Some(r) => r.value,
            None => return diag::render_json(&diags, FILE),
        },
    };
    match crate::generate(&prog, crate::Target::Js, FILE) {
        Ok(code) => code,
        Err(msg) => format!("{{\"ok\":false,\"errors\":[],\"internal\":{}}}", json_str(&msg)),
    }
}

/// The options of a run, from the `limits` text.
struct RunOpts {
    cfg: sandbox::Config,
    strict: bool,
}

fn run_opts(limits: &str) -> Result<RunOpts, String> {
    let mut cfg = sandbox::Config::new();
    cfg.limits.steps = FUEL;
    cfg.limits.memory = MEMORY;
    cfg.limits.output = OUTPUT;
    cfg.limits.depth = DEPTH;
    cfg.limits.wall_ms = TIME_MS;
    cfg.confined = true;
    cfg.to_stdout = false;
    let mut strict = false;
    for item in limits.split([' ', ',', '\n', '\t']).filter(|s| !s.is_empty()) {
        let (key, value) = item.split_once('=').ok_or_else(|| format!("limit `{item}` needs a value: `{item}=...`"))?;
        let number = || value.replace('_', "").parse::<u64>().map_err(|_| format!("`{key}` needs a whole number, found `{value}`"));
        match key {
            "fuel" => cfg.limits.steps = number()?,
            "memory" => cfg.limits.memory = sandbox::parse_size(value)?,
            "output" => cfg.limits.output = sandbox::parse_size(value)?,
            "depth" => cfg.limits.depth = (number()? as usize).min(sandbox::MAX_DEPTH),
            "time" => cfg.limits.wall_ms = number()?,
            "strict" => strict = value != "0",
            _ => return Err(format!("unknown limit `{key}`: use fuel, memory, output, depth, time or strict")),
        }
    }
    Ok(RunOpts { cfg, strict })
}

/// `run_interp`: compiles as `nyra run --sandbox --allow input` does (errors with one certain fix
/// are repaired in memory and listed in `fixed`, unless `strict=1`) and runs the program in the
/// interpreter with `stdin` as its standard input.
pub fn run_json(src: &str, stdin: &str, limits: &str) -> String {
    crate::modules::set_main(None);
    let mut opts = match run_opts(limits) {
        Ok(o) => o,
        Err(msg) => return format!("{{\"ok\":false,\"exit\":2,\"stdout\":\"\",\"errors\":[],\"stderr\":{}}}", json_str(&msg)),
    };
    let start = Instant::now();
    let mut applied = Vec::new();
    let prog = match compile_here(src) {
        Ok(p) => p,
        Err(diags) => {
            let repaired = if opts.strict { None } else { fix::repair(src, diags.clone(), compile_here) };
            match repaired {
                Some(r) => {
                    applied = r.applied;
                    r.value
                }
                None => {
                    return format!(
                        "{{\"ok\":false,\"exit\":1,\"stage\":\"compile\",\"stdout\":\"\",\"errors\":{}{}}}",
                        diag::render_json_errors(&diags, FILE),
                        warnings_field()
                    )
                }
            }
        }
    };
    crate::perfwarn::check(&prog, false);
    let warnings = warnings_field();
    let fixed = if applied.is_empty() { String::new() } else { format!(",\"fixed\":{}", diag::render_json_warnings(&applied, FILE)) };
    let module = match sandbox::module(&prog) {
        Ok(m) => m,
        Err(msg) => return format!("{{\"ok\":false,\"exit\":2,\"stdout\":\"\",\"errors\":[],\"stderr\":{}}}", json_str(&msg)),
    };
    let compile_ms = start.elapsed().as_secs_f64() * 1000.0;
    opts.cfg.stdin = Some(stdin.as_bytes().to_vec());
    let report = sandbox::run(&module, opts.cfg);
    let mut out = format!("{{\"ok\":{},\"exit\":{},\"stdout\":{}", report.exit == 0, report.exit, json_str(&report.stdout));
    if let Some(e) = &report.error {
        out += &format!(
            ",\"errors\":[{{\"code\":\"{}\",\"message\":{},\"file\":{},\"line\":{},\"col\":{},\"hint\":{},\"runtime\":true}}]",
            e.code,
            json_str(&e.msg),
            json_str(FILE),
            e.span.line,
            e.span.col,
            json_str(e.hint)
        );
    }
    if let Some(what) = &report.internal {
        out += &format!(",\"stderr\":{}", json_str(&format!("internal error in the interpreter: {what}")));
    }
    out += &warnings;
    out += &fixed;
    out += &format!(",\"steps\":{},\"ms\":{{\"compile\":{:.1},\"run\":{:.1}}}}}", report.steps, compile_ms, report.run_ms);
    out
}

/// `explain`: one entry as JSON with its text for people, or the list of codes for `""`.
pub fn explain_json(code: &str) -> String {
    let all = match explain::database() {
        Ok(a) => a,
        Err(e) => return format!("{{\"ok\":false,\"error\":{}}}", json_str(&e)),
    };
    if code.trim().is_empty() {
        return explain::list_json(&all, false);
    }
    let code = explain::normalize(code);
    match all.iter().find(|e| e.code == code) {
        Some(e) => {
            let entry = explain::entry_json(e);
            format!("{{\"ok\":true,{},\"text\":{}}}", &entry[1..entry.len() - 1], json_str(&explain::render_entry(e, &all)))
        }
        None => explain::unknown_json(&code, explain::closest(&code, &all)),
    }
}

/// `fmt`: the canonical form of a program that compiles (certain fixes applied first), as `nyra fmt`.
pub fn fmt_json(src: &str) -> String {
    crate::modules::set_main(None);
    let compile = |s: &str| crate::compile_granted(s, &caps::Grant::all(), &flag);
    let (text, applied) = match compile(src) {
        Ok(_) => (src.to_string(), Vec::new()),
        Err(diags) => match fix::repair(src, diags.clone(), compile) {
            Some(r) => (r.text, r.applied),
            None => return diag::render_json(&diags, FILE),
        },
    };
    let formatted = fmt::format(&text);
    if let Err(why) = fmt::verify(&text, &formatted) {
        return format!(
            "{{\"ok\":false,\"errors\":[],\"internal\":{}}}",
            json_str(&format!("formatting would change the program ({why}); please report this bug"))
        );
    }
    format!(
        "{{\"ok\":true,\"source\":{},\"changed\":{},\"fixed\":{}}}",
        json_str(&formatted),
        formatted != src,
        diag::render_json_warnings(&applied, FILE)
    )
}

/// `fix`: applies the fixes that are certain, round after round as `nyra check --fix` does (fixing
/// the syntax can uncover a type error with its own fix). Unlike `--fix` it keeps going when an
/// error has no fix: those are listed in `errors`, and `ok` is true only when none is left.
pub fn fix_json(src: &str) -> String {
    crate::modules::set_main(None);
    let errors = |text: &str| compile_here(text).err().unwrap_or_default();
    let mut text = src.to_string();
    let mut left = errors(&text);
    let mut applied = Vec::new();
    for _ in 0..10 {
        // the fixes that go together (one that overlaps an earlier one waits for the next round)
        let mut edits = Vec::new();
        let mut round = Vec::new();
        for d in left.iter().filter(|d| !d.fix.is_empty()) {
            let mut with = edits.clone();
            with.extend(d.fix.iter().cloned());
            if fix::apply(&text, &with).is_some() {
                edits = with;
                let line = d.span.line.checked_sub(1).and_then(|i| text.lines().nth(i)).unwrap_or("").to_string();
                round.push(fix::Applied { diag: d.clone(), preview: fix::preview(&text, &d.fix), line });
            }
        }
        let Some(next) = fix::apply(&text, &edits).filter(|_| !edits.is_empty()) else { break };
        applied.extend(round);
        text = next;
        left = errors(&text);
        if left.is_empty() {
            break;
        }
    }
    format!(
        "{{\"ok\":{},\"source\":{},\"fixed\":{},\"errors\":{}}}",
        left.is_empty(),
        json_str(&text),
        diag::render_json_warnings(&applied, FILE),
        diag::render_json_errors(&left, FILE)
    )
}

// ---- the C ABI -----------------------------------------------------------------------------------

thread_local! {
    /// The result of the last call (see the module comment).
    static RESULT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

fn give(text: String) -> *const u8 {
    RESULT.with(|r| {
        let mut r = r.borrow_mut();
        *r = text.into_bytes();
        r.as_ptr()
    })
}

/// The text at `(ptr, len)`; invalid UTF-8 is replaced, a null pointer is empty.
///
/// # Safety
/// `ptr` points to `len` readable bytes (or is null).
unsafe fn text<'a>(ptr: *const u8, len: usize) -> std::borrow::Cow<'a, str> {
    if ptr.is_null() || len == 0 {
        return "".into();
    }
    String::from_utf8_lossy(std::slice::from_raw_parts(ptr, len))
}

/// A buffer of `len` bytes for the page to write into.
#[cfg_attr(target_arch = "wasm32", no_mangle)]
pub extern "C" fn nyra_alloc(len: usize) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(len.max(1));
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// Frees a buffer of `nyra_alloc`.
///
/// # Safety
/// `ptr` and `len` are those of one `nyra_alloc` call, freed once.
#[cfg_attr(target_arch = "wasm32", no_mangle)]
pub unsafe extern "C" fn nyra_free(ptr: *mut u8, len: usize) {
    if !ptr.is_null() {
        drop(Vec::from_raw_parts(ptr, 0, len.max(1)));
    }
}

/// The length in bytes of the last result.
#[cfg_attr(target_arch = "wasm32", no_mangle)]
pub extern "C" fn result_len() -> usize {
    RESULT.with(|r| r.borrow().len())
}

/// # Safety
/// `(src, len)` is readable UTF-8 (see the module comment).
#[cfg_attr(target_arch = "wasm32", no_mangle)]
pub unsafe extern "C" fn check(src: *const u8, len: usize) -> *const u8 {
    give(check_json(&text(src, len)))
}

/// # Safety
/// As `check`.
#[cfg_attr(target_arch = "wasm32", no_mangle)]
pub unsafe extern "C" fn compile_js(src: *const u8, len: usize) -> *const u8 {
    give(compile_js_text(&text(src, len)))
}

/// # Safety
/// As `check`, for the three strings.
#[cfg_attr(target_arch = "wasm32", no_mangle)]
pub unsafe extern "C" fn run_interp(
    src: *const u8,
    src_len: usize,
    stdin: *const u8,
    stdin_len: usize,
    limits: *const u8,
    limits_len: usize,
) -> *const u8 {
    give(run_json(&text(src, src_len), &text(stdin, stdin_len), &text(limits, limits_len)))
}

/// # Safety
/// As `check`.
#[cfg_attr(target_arch = "wasm32", no_mangle)]
pub unsafe extern "C" fn explain(code: *const u8, len: usize) -> *const u8 {
    give(explain_json(&text(code, len)))
}

/// # Safety
/// As `check`.
#[cfg_attr(target_arch = "wasm32", no_mangle)]
pub unsafe extern "C" fn fmt(src: *const u8, len: usize) -> *const u8 {
    give(fmt_json(&text(src, len)))
}

/// # Safety
/// As `check`.
#[cfg_attr(target_arch = "wasm32", no_mangle)]
pub unsafe extern "C" fn fix(src: *const u8, len: usize) -> *const u8 {
    give(fix_json(&text(src, len)))
}

#[cfg_attr(target_arch = "wasm32", no_mangle)]
pub extern "C" fn version() -> *const u8 {
    give(env!("CARGO_PKG_VERSION").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::Json;

    /// Calls an exported function the way the page does: through a buffer and `result_len`.
    fn call(f: unsafe extern "C" fn(*const u8, usize) -> *const u8, arg: &str) -> String {
        let p = nyra_alloc(arg.len());
        unsafe {
            std::ptr::copy_nonoverlapping(arg.as_ptr(), p, arg.len());
            let r = f(p, arg.len());
            let out = String::from_utf8(std::slice::from_raw_parts(r, result_len()).to_vec()).unwrap();
            nyra_free(p, arg.len());
            out
        }
    }

    fn run(src: &str, stdin: &str, limits: &str) -> Json {
        let bufs: Vec<(*mut u8, usize)> = [src, stdin, limits]
            .iter()
            .map(|s| {
                let p = nyra_alloc(s.len());
                unsafe { std::ptr::copy_nonoverlapping(s.as_ptr(), p, s.len()) };
                (p, s.len())
            })
            .collect();
        let out = unsafe {
            let r = run_interp(bufs[0].0, bufs[0].1, bufs[1].0, bufs[1].1, bufs[2].0, bufs[2].1);
            String::from_utf8(std::slice::from_raw_parts(r, result_len()).to_vec()).unwrap()
        };
        for (p, n) in bufs {
            unsafe { nyra_free(p, n) };
        }
        Json::parse(&out).unwrap_or_else(|e| panic!("{e}: {out}"))
    }

    fn json(text: &str) -> Json {
        Json::parse(text).unwrap_or_else(|e| panic!("{e}: {text}"))
    }

    /// The entry points need the big stack of the compiler's thread, as the CLI's do.
    fn big_stack(f: impl FnOnce() + Send) {
        std::thread::scope(|s| std::thread::Builder::new().stack_size(crate::STACK).spawn_scoped(s, f).unwrap().join().unwrap());
    }

    #[test]
    fn check_reports_errors_with_fixes() {
        big_stack(|| {
            assert_eq!(json(&call(check, "print(1 + 2)\n")).get("ok"), Some(&Json::Bool(true)));
            let out = json(&call(check, "let x = 1\nprint(y)\n"));
            assert_eq!(out.get("ok"), Some(&Json::Bool(false)));
            let errs = out.get("errors").and_then(Json::as_array).unwrap();
            assert_eq!(errs[0].get("code").and_then(Json::as_str), Some("E0201"));
            assert_eq!(errs[0].get("line").and_then(Json::as_f64), Some(2.0));
        });
    }

    #[test]
    fn run_prints_reads_stdin_and_stops_at_limits() {
        big_stack(|| {
            let out = run("use input\nlet name = input.line()\nprint(\"hi {name}\")\n", "Ada\n", "");
            assert_eq!(out.get("stdout").and_then(Json::as_str), Some("hi Ada\n"), "{out}");
            assert_eq!(out.get("exit").and_then(Json::as_f64), Some(0.0));

            let out = run("var i = 0\nwhile true {\n    i += 1\n}\n", "", "fuel=10000");
            assert_eq!(out.get("exit").and_then(Json::as_f64), Some(120.0), "{out}");
            let e = &out.get("errors").and_then(Json::as_array).unwrap()[0];
            assert_eq!(e.get("code").and_then(Json::as_str), Some("E0355"));

            let out = run("let xs = [1, 2]\nprint(xs[5])\n", "", "");
            assert_eq!(out.get("exit").and_then(Json::as_f64), Some(101.0), "{out}");
            assert_eq!(out.get("errors").and_then(Json::as_array).unwrap()[0].get("line").and_then(Json::as_f64), Some(2.0));
        });
    }

    #[test]
    fn the_playground_grants_only_input() {
        big_stack(|| {
            for m in ["fs", "os"] {
                let out = run(&format!("use {m}\nprint(1)\n"), "", "");
                let e = &out.get("errors").and_then(Json::as_array).unwrap()[0];
                assert_eq!(e.get("code").and_then(Json::as_str), Some("E0290"), "{out}");
                assert!(e.get("hint").and_then(Json::as_str).unwrap().contains("--allow"), "{out}");
                assert_eq!(out.get("stage").and_then(Json::as_str), Some("compile"));
            }
        });
    }

    #[test]
    fn js_fmt_fix_explain_and_version() {
        big_stack(|| {
            let js = call(compile_js, "let n = 40\nprint(n + 2)\n");
            assert!(!js.starts_with('{') && js.contains("40"), "{js}");
            assert!(call(compile_js, "print(nope)\n").starts_with("{\"ok\":false"));

            let out = json(&call(fmt, "fn f(x: int) -> int {\nret x * 2\n}\nprint(f(2))\n"));
            assert_eq!(out.get("source").and_then(Json::as_str), Some("fn f(x: int) -> int {\n    return x * 2\n}\nprint(f(2))\n"));

            let explained = json(&call(explain, "e201"));
            assert_eq!(explained.get("code").and_then(Json::as_str), Some("E0201"));
            assert!(explained.get("text").and_then(Json::as_str).unwrap().contains("What it means"));
            assert!(json(&call(explain, "")).get("codes").is_some());

            assert_eq!(
                unsafe { String::from_utf8_lossy(std::slice::from_raw_parts(version(), result_len())).to_string() },
                env!("CARGO_PKG_VERSION")
            );
        });
    }

    #[test]
    fn fix_applies_the_certain_fixes() {
        big_stack(|| {
            // check is strict: the error is shown, with its fix
            let src = "let xs = [1, 2]\nxs.append(3)\nprint(len(xs));\n";
            let out = json(&check_json(src));
            assert_eq!(out.get("ok"), Some(&Json::Bool(false)), "{out}");
            assert!(out.get("errors").and_then(Json::as_array).unwrap()[0].get("fix").is_some(), "{out}");

            let out = json(&call(fix, src));
            assert_eq!(out.get("ok"), Some(&Json::Bool(true)), "{out}");
            assert_eq!(out.get("source").and_then(Json::as_str), Some("var xs = [1, 2]\nxs.push(3)\nprint(xs.len())\n"));

            // run repairs in memory, as `nyra run` does, and says what it fixed; strict=1 does not
            let ran = run(src, "", "");
            assert_eq!(ran.get("stdout").and_then(Json::as_str), Some("3\n"), "{ran}");
            assert!(ran.get("fixed").and_then(Json::as_array).is_some_and(|f| !f.is_empty()), "{ran}");
            assert_eq!(run(src, "", "strict=1").get("stage").and_then(Json::as_str), Some("compile"));

            // an error without a fix stays, the fixes of the others are applied
            let out = json(&call(fix, "let xs = [1]\nxs.append(2)\nprint(cout)\n"));
            assert_eq!(out.get("ok"), Some(&Json::Bool(false)), "{out}");
            assert_eq!(out.get("source").and_then(Json::as_str), Some("var xs = [1]\nxs.push(2)\nprint(cout)\n"), "{out}");
        });
    }
}
