//! Mocked capabilities: `nyra run --mock fs=DIR`, `--mock input=FILE`, `--mock os.env=KEY=VAL`.
//!
//! A mock grants its capability, but only inside the mock: the program runs in the interpreter and
//! every call of the module goes to the mock instead of the machine.
//!
//! - `fs=DIR`: a copy-on-write view of DIR. Reads see DIR's files; writes, appends, new folders
//!   and removals go to an overlay in memory, so DIR and the rest of the disk never change. Paths
//!   are relative to DIR; an absolute path or one with `..` is error E0340 (outside the mock).
//! - `input=FILE`: the program's standard input is the file's bytes.
//! - `os.env=KEY=VAL` (repeatable): `os.env` and `os.has_env` see only these variables.
//!
//! Like `--allow`, a run with mocks starts from no capability: the mocked ones are granted, and
//! `--allow` may add others. The design of network mocks (`--mock net=DIR`, once Nyra has a `net`
//! module) is in docs/SERVE.md.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::rc::Rc;

use crate::ir::interp::Value;
use crate::ir::StdFn;

/// What the overlay says about a path.
#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    File(String),
    Dir,
    /// Removed (DIR may still have it: it is hidden).
    Gone,
}

/// A copy-on-write view of a folder.
#[derive(Clone, Debug)]
pub struct MockFs {
    pub root: PathBuf,
    /// Paths relative to `root` (parts joined with `/`) that the program changed.
    pub over: BTreeMap<String, Entry>,
}

/// How an operation failed: the operation's words and the reason, as `fs_fail` in `ir/host.rs`
/// words the errors of the real files.
pub type Failure = (&'static str, &'static str);

const OUTSIDE: &str = "outside the mock folder (--mock fs gives the program only the paths below it)";

impl MockFs {
    pub fn new(root: PathBuf) -> MockFs {
        MockFs { root, over: BTreeMap::new() }
    }

    /// The path relative to the root, `""` for the root itself; `None` if it leaves the mock.
    fn rel(path: &str) -> Option<String> {
        if path.starts_with(['/', '\\']) || path.contains(':') {
            return None;
        }
        let mut parts = Vec::new();
        for p in path.split(['/', '\\']) {
            match p {
                "" | "." => {}
                ".." => return None,
                p => parts.push(p),
            }
        }
        Some(parts.join("/"))
    }

    fn parent(rel: &str) -> &str {
        rel.rfind('/').map_or("", |i| &rel[..i])
    }

    /// 0: nothing, 1: a file, 2: a folder.
    fn kind(&self, rel: &str) -> u8 {
        if rel.is_empty() {
            return 2;
        }
        match self.over.get(rel) {
            Some(Entry::File(_)) => 1,
            Some(Entry::Dir) => 2,
            Some(Entry::Gone) => 0,
            None => match std::fs::metadata(self.root.join(rel)) {
                Ok(m) if m.is_dir() => 2,
                Ok(_) => 1,
                Err(_) => 0,
            },
        }
    }

    fn read(&self, rel: &str) -> Result<String, &'static str> {
        match self.kind(rel) {
            0 => return Err("not found"),
            2 => return Err("is a directory"),
            _ => {}
        }
        if let Some(Entry::File(t)) = self.over.get(rel) {
            return Ok(t.clone());
        }
        let bytes = std::fs::read(self.root.join(rel)).map_err(|_| "io error")?;
        String::from_utf8(bytes).map_err(|_| "not valid UTF-8")
    }

    fn list(&self, rel: &str) -> Result<Vec<String>, &'static str> {
        match self.kind(rel) {
            0 => return Err("not found"),
            1 => return Err("not a directory"),
            _ => {}
        }
        let mut names = BTreeSet::new();
        if !matches!(self.over.get(rel), Some(Entry::Dir)) {
            for entry in std::fs::read_dir(self.root.join(rel)).into_iter().flatten().flatten() {
                match entry.file_name().into_string() {
                    Ok(n) => names.insert(n),
                    Err(_) => return Err("not valid UTF-8"),
                };
            }
        }
        for (k, e) in &self.over {
            if Self::parent(k) != rel || k.is_empty() {
                continue;
            }
            let name = k.rsplit('/').next().unwrap_or(k).to_string();
            if *e == Entry::Gone {
                names.remove(&name);
            } else {
                names.insert(name);
            }
        }
        Ok(names.into_iter().collect())
    }

    /// A file written into the overlay: its folder must exist, and it must not be a folder.
    fn put(&mut self, rel: &str, text: &str, append: bool) -> Result<(), &'static str> {
        if rel.is_empty() {
            return Err("is a directory");
        }
        match self.kind(rel) {
            2 => return Err("is a directory"),
            _ if self.kind(Self::parent(rel)) != 2 => return Err("not found"),
            _ => {}
        }
        let mut content = if append && self.kind(rel) == 1 { self.read(rel)? } else { String::new() };
        content.push_str(text);
        self.over.insert(rel.to_string(), Entry::File(content));
        Ok(())
    }

    /// One `fs` call: its result and the steps it costs (bytes read or written), or why it failed.
    pub fn call(&mut self, f: StdFn, path: &str, text: Option<&str>) -> Result<(Option<Value>, u64), Failure> {
        let what = match f {
            StdFn::FsRead => "fs.read: cannot read",
            StdFn::FsWrite => "fs.write: cannot write",
            StdFn::FsAppend => "fs.append: cannot append to",
            StdFn::FsExists => "fs.exists: cannot test",
            StdFn::FsList => "fs.list: cannot list",
            StdFn::FsRemove => "fs.remove: cannot remove",
            _ => "fs.mkdir: cannot create",
        };
        let Some(rel) = Self::rel(path) else { return Err((what, OUTSIDE)) };
        // (like the real files: an empty path or one with a NUL names nothing)
        let nothing = path.is_empty() || path.contains('\0');
        let str_value = |t: String| Some(Value::Str(Rc::new(t)));
        match f {
            StdFn::FsRead => {
                if nothing {
                    return Err((what, "not found"));
                }
                let t = self.read(&rel).map_err(|r| (what, r))?;
                let cost = t.len() as u64;
                Ok((str_value(t), cost))
            }
            StdFn::FsWrite | StdFn::FsAppend => {
                if nothing {
                    return Err((what, "not found"));
                }
                let t = text.unwrap_or("");
                self.put(&rel, t, f == StdFn::FsAppend).map_err(|r| (what, r))?;
                Ok((None, t.len() as u64))
            }
            StdFn::FsExists => Ok((Some(Value::Bool(!nothing && self.kind(&rel) != 0)), 0)),
            StdFn::FsList => {
                if nothing {
                    return Err((what, "not found"));
                }
                let names = self.list(&rel).map_err(|r| (what, r))?;
                let cost = names.iter().map(|n| n.len() as u64 + 1).sum();
                Ok((Some(Value::arr(names.into_iter().map(|n| Value::Str(Rc::new(n))).collect())), cost))
            }
            StdFn::FsRemove => {
                if nothing || rel.is_empty() {
                    return Err((what, if nothing { "not found" } else { "permission denied" }));
                }
                match self.kind(&rel) {
                    0 => return Err((what, "not found")),
                    2 if !self.list(&rel).map_err(|r| (what, r))?.is_empty() => return Err((what, "not empty")),
                    _ => {}
                }
                self.over.insert(rel, Entry::Gone);
                Ok((None, 0))
            }
            StdFn::FsMkdir => {
                if nothing {
                    return Err((what, "not found"));
                }
                if self.kind(&rel) != 0 {
                    return Err((what, "already exists"));
                }
                if self.kind(Self::parent(&rel)) != 2 {
                    return Err((what, "not found"));
                }
                self.over.insert(rel, Entry::Dir);
                Ok((None, 0))
            }
            _ => Err((what, "io error")),
        }
    }
}

/// The mocks of a run, from the `--mock` flags.
#[derive(Default, Clone, Debug)]
pub struct Mocks {
    pub fs: Option<PathBuf>,
    pub input: Option<PathBuf>,
    pub env: Option<Vec<(String, String)>>,
}

impl Mocks {
    /// Adds one `--mock` value: `fs=DIR`, `input=FILE` or `os.env=KEY=VAL`.
    pub fn add(&mut self, spec: &str) -> Result<(), String> {
        let usage = "--mock takes fs=DIR, input=FILE or os.env=KEY=VALUE (net=DIR comes with networking)";
        let Some((what, value)) = spec.split_once('=') else { return Err(format!("`--mock {spec}`: {usage}")) };
        match what {
            "fs" => {
                let dir = PathBuf::from(value);
                if !dir.is_dir() {
                    return Err(format!("--mock fs={value}: `{value}` is not a folder"));
                }
                self.fs = Some(dir);
            }
            "input" => {
                if !std::path::Path::new(value).is_file() {
                    return Err(format!("--mock input={value}: `{value}` is not a file"));
                }
                self.input = Some(PathBuf::from(value));
            }
            "os.env" | "env" => {
                let Some((k, v)) = value.split_once('=') else {
                    return Err(format!("--mock os.env={value}: write the variable as KEY=VALUE"));
                };
                if k.is_empty() {
                    return Err(format!("--mock os.env={value}: the variable has no name"));
                }
                let env = self.env.get_or_insert_with(Vec::new);
                env.retain(|(name, _)| name != k);
                env.push((k.to_string(), v.to_string()));
            }
            "net" => return Err("--mock net: Nyra has no network module yet (the design is in docs/SERVE.md)".to_string()),
            _ => return Err(format!("`--mock {spec}`: unknown mock `{what}`; {usage}")),
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.fs.is_none() && self.input.is_none() && self.env.is_none()
    }

    /// The capabilities the mocks grant.
    pub fn caps(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if self.fs.is_some() {
            v.push("fs");
        }
        if self.input.is_some() {
            v.push("input");
        }
        if self.env.is_some() {
            v.push("os");
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nyra-mock-unit-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::write(d.join("a.txt"), "hello").unwrap();
        std::fs::write(d.join("sub/b.txt"), "b").unwrap();
        d
    }

    fn text(r: Result<(Option<Value>, u64), Failure>) -> String {
        match r {
            Ok((Some(Value::Str(s)), _)) => s.to_string(),
            Ok((Some(Value::Arr(xs)), _)) => {
                xs.iter().map(|v| if let Value::Str(s) = v { s.to_string() } else { "?".into() }).collect::<Vec<_>>().join(",")
            }
            Ok((Some(Value::Bool(b)), _)) => b.to_string(),
            Ok(_) => "ok".into(),
            Err((_, why)) => format!("error: {why}"),
        }
    }

    #[test]
    fn writes_stay_in_memory_and_reads_see_them() {
        let d = dir("cow");
        let mut fs = MockFs::new(d.clone());
        assert_eq!(text(fs.call(StdFn::FsRead, "a.txt", None)), "hello");
        assert_eq!(text(fs.call(StdFn::FsAppend, "a.txt", Some(" world"))), "ok");
        assert_eq!(text(fs.call(StdFn::FsRead, "./a.txt", None)), "hello world");
        assert_eq!(std::fs::read_to_string(d.join("a.txt")).unwrap(), "hello", "the folder never changes");
        assert_eq!(text(fs.call(StdFn::FsMkdir, "new", None)), "ok");
        assert_eq!(text(fs.call(StdFn::FsWrite, "new/c.txt", Some("c"))), "ok");
        assert!(!d.join("new").exists());
        assert_eq!(text(fs.call(StdFn::FsList, ".", None)), "a.txt,new,sub");
        assert_eq!(text(fs.call(StdFn::FsRemove, "sub/b.txt", None)), "ok");
        assert_eq!(text(fs.call(StdFn::FsExists, "sub/b.txt", None)), "false");
        assert!(d.join("sub/b.txt").exists());
        assert_eq!(text(fs.call(StdFn::FsList, "sub", None)), "");
        assert_eq!(text(fs.call(StdFn::FsRemove, "sub", None)), "ok");
        assert_eq!(text(fs.call(StdFn::FsList, "sub/..", None)), format!("error: {OUTSIDE}"));
        assert_eq!(text(fs.call(StdFn::FsList, "./", None)), "a.txt,new");
        assert_eq!(text(fs.call(StdFn::FsRemove, "new", None)), "error: not empty");
        assert_eq!(text(fs.call(StdFn::FsWrite, "nope/x.txt", Some("x"))), "error: not found");
        assert_eq!(text(fs.call(StdFn::FsRead, "../secret", None)), format!("error: {OUTSIDE}"));
        assert_eq!(text(fs.call(StdFn::FsRead, "/etc/passwd", None)), format!("error: {OUTSIDE}"));
        assert_eq!(text(fs.call(StdFn::FsRead, "sub", None)), "error: not found");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn mock_flags_parse() {
        let mut m = Mocks::default();
        m.add("os.env=HOME=/home/x").unwrap();
        m.add("os.env=A=1=2").unwrap();
        assert_eq!(m.env.as_ref().unwrap()[1], ("A".to_string(), "1=2".to_string()));
        assert!(m.add("net=fixtures").unwrap_err().contains("no network module"));
        assert!(m.add("disk=x").unwrap_err().contains("unknown mock"));
        assert!(m.add("fs=/definitely/not/here").is_err());
        assert_eq!(m.caps(), ["os"]);
    }
}
