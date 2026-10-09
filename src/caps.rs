//! Capabilities: what a program may touch outside of itself, decided before it runs.
//!
//! A program says what it touches with its `use` lines, and the standard modules fall in two
//! groups:
//!
//! - *pure* modules, always available: `json`, `math`, `text`, and `time` and `random` (they only
//!   read a clock or a generator; nothing outside the program changes);
//! - *effectful* modules, which need a capability of the same name: `fs` (files and folders),
//!   `input` (standard input), `os` (arguments, environment variables, `exit`), and `net` when
//!   the language gets networking. Any module that is not listed as pure needs one, so a module
//!   added later is closed by default.
//!
//! The CLI and the MCP server decide which capabilities a run grants. A `use` of a module whose
//! capability is not granted is a compile error (E0290) that names the module, the capability and
//! the flag to add, so an agent that runs code unsupervised can hand out exactly what a task needs
//! and learns at compile time, not halfway through a run, what it lacks.

use crate::ast::Program;
use crate::diag::Diag;

/// Modules every program may use.
pub const PURE: &[&str] = &["json", "math", "random", "text", "time"];

/// The capabilities that can be granted, sorted, with what each one allows.
pub const CAPABILITIES: &[(&str, &str)] = &[
    ("fs", "read, write, list and remove files and folders"),
    ("input", "read standard input"),
    ("net", "network access (no standard module uses it yet)"),
    ("os", "read arguments and environment variables, stop the program with `exit`"),
];

/// The capability a standard module needs, or `None` for a pure module.
pub fn needed(module: &str) -> Option<&'static str> {
    if PURE.contains(&module) {
        return None;
    }
    CAPABILITIES.iter().map(|(c, _)| *c).find(|c| *c == module).or(Some("net"))
}

/// "`fs`, `input`, `net` and `os`"
pub fn list() -> String {
    let names: Vec<String> = CAPABILITIES.iter().map(|(c, _)| format!("`{c}`")).collect();
    format!("{} and {}", names[..names.len() - 1].join(", "), names[names.len() - 1])
}

/// The capabilities a run grants.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grant {
    all: bool,
    caps: Vec<String>,
}

impl Grant {
    /// Everything (`nyra run` without `--sandbox` or `--allow`).
    pub fn all() -> Grant {
        Grant { all: true, caps: Vec::new() }
    }

    /// Nothing but the pure modules.
    pub fn none() -> Grant {
        Grant { all: false, caps: Vec::new() }
    }

    /// Adds the capabilities of an `--allow` value: names separated by commas, `all`, or `none`.
    /// An unknown name is an error that says what exists.
    pub fn allow(&mut self, spec: &str) -> Result<(), String> {
        for name in spec.split(',').map(str::trim).filter(|n| !n.is_empty()) {
            match name {
                "all" => self.all = true,
                "none" => {}
                _ if CAPABILITIES.iter().any(|(c, _)| *c == name) => {
                    if !self.caps.iter().any(|c| c == name) {
                        self.caps.push(name.to_string());
                    }
                }
                _ => {
                    let hint = match crate::diag::suggest(name, CAPABILITIES.iter().map(|(c, _)| *c)) {
                        Some(s) => format!(" {s}"),
                        None => String::new(),
                    };
                    return Err(format!("unknown capability `{name}`: the capabilities are {} (or `all`){hint}", list()));
                }
            }
        }
        Ok(())
    }

    pub fn allows(&self, cap: &str) -> bool {
        self.all || self.caps.iter().any(|c| c == cap)
    }

    /// The granted names, sorted (`["fs", "input", "net", "os"]` for all).
    pub fn names(&self) -> Vec<String> {
        CAPABILITIES.iter().map(|(c, _)| c.to_string()).filter(|c| self.allows(c)).collect()
    }
}

/// E0290 for every `use` of a module whose capability `grant` does not give. `flag` says how to
/// grant one in the place the run was started from (`--allow fs` for the CLI, an `allow` argument
/// for the MCP tools): a sentence that starts with a verb.
pub fn enforce(prog: &Program, grant: &Grant, flag: impl Fn(&str) -> String) -> Vec<Diag> {
    let mut errs = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for u in &prog.uses {
        let Some(cap) = needed(&u.module) else { continue };
        if grant.allows(cap) || seen.contains(&u.module.as_str()) {
            continue;
        }
        seen.push(&u.module);
        let what = CAPABILITIES.iter().find(|(c, _)| *c == cap).map_or("", |(_, w)| *w);
        let granted = grant.names();
        let now = if granted.is_empty() { "this run grants none".to_string() } else { format!("this run grants {}", granted.join(", ")) };
        errs.push(
            Diag::new(
                "E0290",
                format!("module `{}` needs the capability `{cap}` ({what}), which is not granted: {now}", u.module),
                u.span,
            )
            .hint(format!("{}, or remove `use {}` and the code that calls it; the capabilities are {}", flag(cap), u.module, list())),
        );
    }
    errs
}

/// The capabilities that the `use` lines of a source text name, for the lines that start with
/// `use` (the outline works on files that do not parse).
pub fn used_modules(text: &str) -> Vec<String> {
    let mut v = Vec::new();
    for line in text.lines() {
        let mut words = line.split_whitespace();
        if words.next() == Some("use") {
            if let Some(m) = words.next() {
                let m = m.trim_end_matches(';');
                if needed(m).is_some() && !v.iter().any(|x| x == m) {
                    v.push(m.to_string());
                }
            }
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pure_modules_need_nothing_and_the_rest_is_closed() {
        for m in PURE {
            assert_eq!(needed(m), None, "{m}");
        }
        assert_eq!(needed("fs"), Some("fs"));
        assert_eq!(needed("os"), Some("os"));
        assert_eq!(needed("input"), Some("input"));
        assert_eq!(needed("some_future_module"), Some("net"));
    }

    #[test]
    fn every_standard_module_is_pure_or_has_a_capability() {
        for m in crate::stdlib::MODULES {
            match needed(m) {
                None => assert!(PURE.contains(m)),
                Some(c) => assert!(CAPABILITIES.iter().any(|(n, _)| *n == c), "{m} -> {c}"),
            }
        }
    }

    #[test]
    fn allow_parses_lists_and_rejects_typos() {
        let mut g = Grant::none();
        assert!(!g.allows("fs"));
        g.allow("fs, os").unwrap();
        assert!(g.allows("fs") && g.allows("os") && !g.allows("input"));
        let e = g.allow("inputs").unwrap_err();
        assert!(e.contains("unknown capability `inputs`") && e.contains("did you mean `input`"), "{e}");
        g.allow("all").unwrap();
        assert!(g.allows("net"));
        assert_eq!(g.names(), ["fs", "input", "net", "os"]);
    }
}
