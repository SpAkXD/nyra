//! Fuel and checkpoints: `nyra run --checkpoint state.json` and `nyra resume state.json`.
//!
//! A run with `--checkpoint` is interpreted (`ir/interp.rs`) and may pause: when its fuel
//! (`--fuel N`) or its time (`--max-time MS`) runs out, it stops before the next statement,
//! instead of failing with E0355 or E0359, and this file saves where it stood as JSON:
//!
//! - `stack`: every call that runs, outermost first: the function, its locals, and the position in
//!   its body (`at`: the statement of each block it is inside, the branch of an `if`, the part of a
//!   loop, the items a `for` has left). The innermost statement has not started yet.
//! - `heap`: the strings, arrays, structs and maps the locals hold, each once; a value that two
//!   locals share is one entry, so the copy-on-write behavior (and the step count) goes on exactly
//!   as it would have.
//! - `host`: what the program's world was: the arguments, the standard input that is left, the
//!   random generator, the virtual clock of `time.sleep_ms`, the mocks (`--mock`).
//! - `source_hash` and `program_hash`: `nyra resume` refuses a checkpoint whose source file, or
//!   whose compiled program (an imported file, another nyra), is not the same any more.
//!
//! The run exits with code 125 (`sandbox::PAUSED_EXIT`). `nyra resume state.json [--fuel N]`
//! goes on from the statement it stopped before, with fresh fuel (by default as much as the run
//! had), and writes the file again if it pauses again. What the program did before the pause
//! (printed text, written files) is not done again. A resumed run that ends removes the file, so
//! the same checkpoint cannot be resumed twice by mistake.

use std::collections::HashMap;
use std::path::Path;
use std::process::ExitCode;
use std::rc::Rc;

use crate::ir::host::HostState;
use crate::ir::interp::{LoopPart, MapVal, Pos, Value};
use crate::ir::{FuncId, Module, Ty};
use crate::json::{obj, Json};
use crate::mock::{Entry, MockFs};
use crate::sandbox::{self, Paused};

/// The version of the format.
const FORMAT: i64 = 1;

/// How a run was started: what `nyra resume` starts it with again.
#[derive(Clone, Debug)]
pub struct Run {
    pub file: String,
    pub fuel: u64,
    pub max_memory: u64,
    pub max_output: u64,
    pub max_depth: usize,
    /// 0: none.
    pub max_time: u64,
    /// The capabilities granted.
    pub allow: Vec<String>,
    pub sandbox: bool,
}

/// A checkpoint read back: what `sandbox::run` needs to go on.
pub struct Loaded {
    pub host: Option<HostState>,
    pub written: u64,
    /// Steps of the runs before.
    pub steps: u64,
    pub run: Run,
    stack: Json,
    heap: Json,
}

// ---- writing ------------------------------------------------------------------------------------

/// FNV-1a of a text, as 16 hex digits.
pub fn hash(text: &str) -> String {
    format!("{:016x}", crate::fnv1a(&[text.as_bytes()]))
}

/// The hash of a compiled program (its IR as text).
pub fn program_hash(m: &Module) -> String {
    hash(&crate::ir::print::print(m))
}

struct Enc<'a> {
    m: &'a Module,
    /// The string literals of the run (a value that is one stays one).
    lits: HashMap<usize, usize>,
    /// Heap objects already written: their address and their index.
    seen: HashMap<usize, usize>,
    heap: Vec<Json>,
}

fn addr<T>(rc: &Rc<T>) -> usize {
    Rc::as_ptr(rc) as *const () as usize
}

fn float(x: f64) -> Json {
    obj([("f", format!("{x:?}").into())])
}

impl Enc<'_> {
    fn value(&mut self, v: &Value, depth: usize) -> Result<Json, String> {
        if depth > 100_000 {
            return Err("a value is nested too deeply to be saved".into());
        }
        // scalars are written in place, heap values once (by their address) and referred to
        let key = match v {
            Value::Unset => return Ok(Json::Null),
            Value::Int(n) => return Ok(Json::from(*n)),
            Value::Float(x) => return Ok(float(*x)),
            Value::Bool(b) => return Ok(Json::Bool(*b)),
            Value::Char(c) => return Ok(obj([("c", c.to_string().into())])),
            Value::Str(s) => {
                if let Some(k) = self.lits.get(&addr(s)) {
                    return Ok(obj([("lit", Json::from(*k as i64))]));
                }
                addr(s)
            }
            Value::Arr(xs) => addr(xs),
            Value::Struct(_, fs) => addr(fs),
            Value::Map(mv) => addr(mv),
        };
        if let Some(i) = self.seen.get(&key) {
            return Ok(obj([("r", Json::from(*i as i64))]));
        }
        // the parts first: an entry refers only to entries before it
        let entry = match v {
            Value::Str(s) => obj([("s", s.as_str().into())]),
            Value::Arr(xs) => obj([("a", self.list(xs, depth)?)]),
            Value::Struct(id, fs) => {
                let name = self.m.structs.get(Ty::Struct(*id)).map_or(String::new(), |s| s.name.clone());
                obj([("t", Json::from(*id as i64)), ("n", name.into()), ("v", self.list(fs, depth)?)])
            }
            Value::Map(mv) => {
                let mut ents = Vec::new();
                for (k, x) in mv.iter() {
                    ents.push(Json::Arr(vec![self.value(k, depth + 1)?, self.value(x, depth + 1)?]));
                }
                obj([("m", Json::Arr(ents))])
            }
            _ => return Err("a value that is not on the heap".into()),
        };
        self.heap.push(entry);
        let i = self.heap.len() - 1;
        self.seen.insert(key, i);
        Ok(obj([("r", Json::from(i as i64))]))
    }

    fn list(&mut self, xs: &[Value], depth: usize) -> Result<Json, String> {
        xs.iter().map(|x| self.value(x, depth + 1)).collect::<Result<Vec<_>, _>>().map(Json::Arr)
    }
}

fn part_name(p: LoopPart) -> &'static str {
    match p {
        LoopPart::Start => "start",
        LoopPart::Head => "head",
        LoopPart::Body => "body",
        LoopPart::Step => "step",
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn host_json(h: &HostState, enc_args: Vec<Json>) -> Json {
    let input = match &h.input {
        None => Json::Null,
        Some(b) => match std::str::from_utf8(b) {
            Ok(t) => t.into(),
            Err(_) => obj([("hex", hex(b).into())]),
        },
    };
    let seed = match h.seeded {
        Some(s) => Json::Arr(s.iter().map(|w| Json::from(*w as i64)).collect()),
        None => Json::Null,
    };
    let mock_fs = match &h.mock_fs {
        None => Json::Null,
        Some(fs) => {
            let over = fs
                .over
                .iter()
                .map(|(k, e)| {
                    let v = match e {
                        Entry::File(t) => obj([("file", t.as_str().into())]),
                        Entry::Dir => "dir".into(),
                        Entry::Gone => "gone".into(),
                    };
                    (k.clone(), v)
                })
                .collect();
            obj([("root", fs.root.to_string_lossy().into_owned().into()), ("over", Json::Obj(over))])
        }
    };
    let mock_env = match &h.mock_env {
        None => Json::Null,
        Some(env) => Json::Arr(env.iter().map(|(k, v)| Json::Arr(vec![k.as_str().into(), v.as_str().into()])).collect()),
    };
    obj([
        ("args", Json::Arr(enc_args)),
        ("input", input),
        ("seed", seed),
        ("os_count", Json::from(h.os_count as i64)),
        ("slept_ms", Json::from(h.slept_ms)),
        ("mono_ms", float(h.mono_ms)),
        ("confined", h.confined.into()),
        ("mock_fs", mock_fs),
        ("mock_env", mock_env),
    ])
}

fn run_json(r: &Run) -> Json {
    obj([
        ("fuel", Json::from(r.fuel as i64)),
        ("max_memory", Json::from(r.max_memory as i64)),
        ("max_output", Json::from(r.max_output as i64)),
        ("max_depth", Json::from(r.max_depth as i64)),
        ("max_time", Json::from(r.max_time as i64)),
        ("allow", Json::Arr(r.allow.iter().map(|a| a.as_str().into()).collect())),
        ("sandbox", r.sandbox.into()),
    ])
}

/// The checkpoint of a paused run as JSON text. `steps`: all the steps it ran, the runs before included.
pub fn encode(m: &Module, run: &Run, source: &str, steps: u64, p: &Paused) -> Result<String, String> {
    let mut enc =
        Enc { m, lits: p.literals.iter().enumerate().map(|(i, a)| (*a, i)).collect(), seen: HashMap::new(), heap: Vec::new() };
    // the trail is innermost first: the frames, outermost first, each with its positions
    let mut stack: Vec<Json> = Vec::new();
    let mut frame: Option<(String, i64, Json, Vec<Json>)> = None;
    let flush = |frame: &mut Option<(String, i64, Json, Vec<Json>)>, stack: &mut Vec<Json>| {
        if let Some((name, id, locals, at)) = frame.take() {
            stack.push(obj([("fn", name.into()), ("id", Json::from(id)), ("locals", locals), ("at", Json::Arr(at))]));
        }
    };
    for pos in p.trail.iter().rev() {
        match pos {
            Pos::Frame(fid, locals) => {
                flush(&mut frame, &mut stack);
                let name = m.funcs.get(fid.0 as usize).map_or(String::new(), |f| f.name.clone());
                frame = Some((name, fid.0 as i64, enc.list(locals, 0)?, Vec::new()));
            }
            other => {
                let Some((_, _, _, at)) = frame.as_mut() else { return Err("a position outside of any call".into()) };
                at.push(match other {
                    Pos::Stmt(i) => obj([("stmt", Json::from(*i as i64))]),
                    Pos::Branch(b) => obj([("branch", (*b).into())]),
                    Pos::Loop(part) => obj([("loop", part_name(*part).into())]),
                    Pos::Each { items, body } => obj([("each", enc.list(items, 0)?), ("body", (*body).into())]),
                    Pos::Frame(..) => unreachable!(),
                });
            }
        }
    }
    flush(&mut frame, &mut stack);
    let args: Vec<Json> = p.host.args.iter().map(|a| a.as_str().into()).collect();
    let doc = obj([
        ("nyra_checkpoint", Json::from(FORMAT)),
        ("nyra", env!("CARGO_PKG_VERSION").into()),
        ("file", run.file.as_str().into()),
        ("source_hash", hash(source).into()),
        ("program_hash", program_hash(m).into()),
        ("steps", Json::from(steps as i64)),
        ("reason", if p.by_time { "time" } else { "fuel" }.into()),
        ("paused_at", obj([("line", Json::from(p.span.line as i64)), ("col", Json::from(p.span.col as i64))])),
        ("written", Json::from(p.written as i64)),
        ("run", run_json(run)),
        ("host", host_json(&p.host, args)),
        ("stack", Json::Arr(stack)),
        ("heap", Json::Arr(enc.heap)),
    ]);
    Ok(pretty(&doc))
}

/// JSON with one entry of the heap and of the stack per line: still one small file, but readable.
fn pretty(doc: &Json) -> String {
    let Json::Obj(fields) = doc else { return doc.to_string() };
    let mut out = String::from("{\n");
    for (i, (k, v)) in fields.iter().enumerate() {
        out.push_str(&format!("  {}: ", crate::diag::json_str(k)));
        match v {
            Json::Arr(items) if (k == "stack" || k == "heap") && !items.is_empty() => {
                out.push_str("[\n");
                for (j, item) in items.iter().enumerate() {
                    out.push_str(&format!("    {item}{}\n", if j + 1 < items.len() { "," } else { "" }));
                }
                out.push_str("  ]");
            }
            v => out.push_str(&v.to_string()),
        }
        out.push_str(if i + 1 < fields.len() { ",\n" } else { "\n" });
    }
    out.push_str("}\n");
    out
}

// ---- reading ------------------------------------------------------------------------------------

fn num(j: Option<&Json>) -> Option<i64> {
    match j {
        Some(Json::Num(n)) => n.parse().ok(),
        _ => None,
    }
}

fn unhex(t: &str) -> Option<Vec<u8>> {
    (0..t.len()).step_by(2).map(|i| t.get(i..i + 2).and_then(|b| u8::from_str_radix(b, 16).ok())).collect()
}

fn parse_float(j: &Json) -> Option<f64> {
    j.get("f")?.as_str()?.parse().ok()
}

/// Reads a checkpoint (its text) back. The trail is made later (`Loaded::trail`): its values need
/// the string literals of the run that goes on.
pub fn decode(text: &str) -> Result<Loaded, String> {
    let doc = Json::parse(text).map_err(|e| format!("it is not valid JSON ({e})"))?;
    if num(doc.get("nyra_checkpoint")) != Some(FORMAT) {
        return Err("it is not a checkpoint of this nyra (no `nyra_checkpoint: 1`)".into());
    }
    let bad = |what: &str| format!("its `{what}` is missing or malformed");
    let r = doc.get("run").ok_or_else(|| bad("run"))?;
    let n = |k: &str| num(r.get(k)).filter(|n| *n >= 0).map(|n| n as u64).ok_or_else(|| bad(&format!("run.{k}")));
    let run = Run {
        file: doc.get("file").and_then(Json::as_str).ok_or_else(|| bad("file"))?.to_string(),
        fuel: n("fuel")?,
        max_memory: n("max_memory")?,
        max_output: n("max_output")?,
        max_depth: n("max_depth")? as usize,
        max_time: n("max_time")?,
        allow: r
            .get("allow")
            .and_then(Json::as_array)
            .ok_or_else(|| bad("run.allow"))?
            .iter()
            .filter_map(|a| a.as_str().map(String::from))
            .collect(),
        sandbox: matches!(r.get("sandbox"), Some(Json::Bool(true))),
    };
    let h = doc.get("host").ok_or_else(|| bad("host"))?;
    let input = match h.get("input") {
        None | Some(Json::Null) => None,
        Some(Json::Str(t)) => Some(t.as_bytes().to_vec()),
        Some(o) => Some(o.get("hex").and_then(Json::as_str).and_then(unhex).ok_or_else(|| bad("host.input"))?),
    };
    let seeded = match h.get("seed") {
        Some(Json::Arr(ws)) if ws.len() == 4 => {
            let mut s = [0u32; 4];
            for (i, w) in ws.iter().enumerate() {
                s[i] = num(Some(w)).and_then(|n| u32::try_from(n).ok()).ok_or_else(|| bad("host.seed"))?;
            }
            Some(s)
        }
        _ => None,
    };
    let mock_fs = match h.get("mock_fs") {
        Some(o @ Json::Obj(_)) => {
            let root = o.get("root").and_then(Json::as_str).ok_or_else(|| bad("host.mock_fs"))?;
            let mut fs = MockFs::new(root.into());
            if let Some(Json::Obj(over)) = o.get("over") {
                for (k, v) in over {
                    let e = match v {
                        Json::Str(s) if s == "dir" => Entry::Dir,
                        Json::Str(s) if s == "gone" => Entry::Gone,
                        o => Entry::File(o.get("file").and_then(Json::as_str).ok_or_else(|| bad("host.mock_fs"))?.to_string()),
                    };
                    fs.over.insert(k.clone(), e);
                }
            }
            Some(fs)
        }
        _ => None,
    };
    let mock_env = match h.get("mock_env") {
        Some(Json::Arr(pairs)) => Some(
            pairs
                .iter()
                .map(|p| match p.as_array() {
                    Some([Json::Str(k), Json::Str(v)]) => Ok((k.clone(), v.clone())),
                    _ => Err(bad("host.mock_env")),
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
        _ => None,
    };
    let host = HostState {
        args: h
            .get("args")
            .and_then(Json::as_array)
            .ok_or_else(|| bad("host.args"))?
            .iter()
            .filter_map(|a| a.as_str().map(String::from))
            .collect(),
        input,
        seeded,
        os_count: num(h.get("os_count")).unwrap_or(0) as u64,
        slept_ms: num(h.get("slept_ms")).unwrap_or(0),
        mono_ms: h.get("mono_ms").and_then(parse_float).unwrap_or(0.0),
        confined: matches!(h.get("confined"), Some(Json::Bool(true))),
        mock_fs,
        mock_env,
    };
    Ok(Loaded {
        host: Some(host),
        written: num(doc.get("written")).unwrap_or(0) as u64,
        steps: num(doc.get("steps")).unwrap_or(0) as u64,
        run,
        stack: doc.get("stack").cloned().ok_or_else(|| bad("stack"))?,
        heap: doc.get("heap").cloned().ok_or_else(|| bad("heap"))?,
    })
}

struct Dec<'a> {
    m: &'a Module,
    lits: &'a [Rc<String>],
    heap: Vec<Value>,
}

impl Dec<'_> {
    fn value(&self, j: &Json) -> Result<Value, String> {
        let bad = || format!("a value of the checkpoint is malformed: {j}");
        Ok(match j {
            Json::Null => Value::Unset,
            Json::Bool(b) => Value::Bool(*b),
            Json::Num(n) => Value::Int(n.parse().map_err(|_| bad())?),
            Json::Obj(_) => {
                if let Some(r) = num(j.get("r")) {
                    self.heap.get(r as usize).cloned().ok_or_else(bad)?
                } else if let Some(k) = num(j.get("lit")) {
                    Value::Str(self.lits.get(k as usize).cloned().ok_or_else(bad)?)
                } else if j.get("f").is_some() {
                    Value::Float(parse_float(j).ok_or_else(bad)?)
                } else if let Some(c) = j.get("c").and_then(Json::as_str) {
                    let mut cs = c.chars();
                    match (cs.next(), cs.next()) {
                        (Some(c), None) => Value::Char(c),
                        _ => return Err(bad()),
                    }
                } else {
                    return Err(bad());
                }
            }
            _ => return Err(bad()),
        })
    }

    fn list(&self, j: Option<&Json>) -> Result<Vec<Value>, String> {
        j.and_then(Json::as_array).ok_or("a list of values is missing")?.iter().map(|x| self.value(x)).collect()
    }

    fn entry(&self, j: &Json) -> Result<Value, String> {
        if let Some(s) = j.get("s").and_then(Json::as_str) {
            return Ok(Value::Str(Rc::new(s.to_string())));
        }
        if j.get("a").is_some() {
            return Ok(Value::arr(self.list(j.get("a"))?));
        }
        if let Some(id) = num(j.get("t")) {
            let name = j.get("n").and_then(Json::as_str).unwrap_or("");
            match self.m.structs.get(Ty::Struct(id as u32)) {
                Some(info) if info.name == name => {}
                _ => return Err(format!("the checkpoint has a struct `{name}` this program does not have")),
            }
            return Ok(Value::strukt(id as u32, self.list(j.get("v"))?));
        }
        if let Some(ents) = j.get("m").and_then(Json::as_array) {
            let mut mv = MapVal::default();
            for e in ents {
                let Some([k, v]) = e.as_array() else { return Err("a map entry of the checkpoint is malformed".into()) };
                mv.set(self.value(k)?, self.value(v)?).map_err(|_| "a map key of the checkpoint is malformed".to_string())?;
            }
            return Ok(Value::Map(Rc::new(mv)));
        }
        Err(format!("a heap entry of the checkpoint is malformed: {j}"))
    }
}

impl Loaded {
    /// The positions of the paused run (innermost first, as `Interp::resume` takes them), with
    /// its values made anew; `lits` are the string literals of the run that goes on.
    pub fn trail(&self, m: &Module, lits: &[Rc<String>]) -> Result<Vec<Pos>, String> {
        let mut dec = Dec { m, lits, heap: Vec::new() };
        for e in self.heap.as_array().ok_or("the heap is not a list")? {
            let v = dec.entry(e)?;
            dec.heap.push(v);
        }
        let mut outer_first = Vec::new();
        for f in self.stack.as_array().ok_or("the stack is not a list")? {
            let id = num(f.get("id")).ok_or("a frame without its function")?;
            let func = m.funcs.get(id as usize).ok_or("a frame of a function this program does not have")?;
            if f.get("fn").and_then(Json::as_str) != Some(func.name.as_str()) {
                return Err(format!(
                    "a frame of `{}`, which this program does not have",
                    f.get("fn").and_then(Json::as_str).unwrap_or("?")
                ));
            }
            outer_first.push(Pos::Frame(FuncId(id as u32), dec.list(f.get("locals"))?));
            for at in f.get("at").and_then(Json::as_array).ok_or("a frame without its position")? {
                outer_first.push(if let Some(i) = num(at.get("stmt")) {
                    Pos::Stmt(i as usize)
                } else if let Some(Json::Bool(b)) = at.get("branch") {
                    Pos::Branch(*b)
                } else if let Some(p) = at.get("loop").and_then(Json::as_str) {
                    Pos::Loop(match p {
                        "start" => LoopPart::Start,
                        "head" => LoopPart::Head,
                        "body" => LoopPart::Body,
                        "step" => LoopPart::Step,
                        _ => return Err(format!("an unknown loop part `{p}`")),
                    })
                } else if at.get("each").is_some() {
                    Pos::Each { items: dec.list(at.get("each"))?, body: matches!(at.get("body"), Some(Json::Bool(true))) }
                } else {
                    return Err(format!("an unknown position {at}"));
                });
            }
        }
        if !matches!(outer_first.first(), Some(Pos::Frame(id, _)) if *id == m.main) {
            return Err("the checkpoint does not start in `main`".into());
        }
        outer_first.reverse();
        Ok(outer_first)
    }
}

// ---- the command line -----------------------------------------------------------------------------

/// After a run with `--checkpoint` paused: writes the checkpoint and says how to go on.
pub fn paused(m: &Module, run: &Run, source: &str, steps_before: u64, report: &sandbox::Report, path: &str, json: bool) -> ExitCode {
    let Some(p) = &report.paused else { return ExitCode::from(report.exit as u8) };
    let steps = steps_before + report.steps;
    let text = match encode(m, run, source, steps, p) {
        Ok(t) => t,
        Err(e) => return crate::fail(format!("cannot save the checkpoint: {e}")),
    };
    if let Err(e) = std::fs::write(path, text) {
        return crate::fail(format!("cannot write the checkpoint `{path}`: {e}"));
    }
    let why = if p.by_time { "its time ran out" } else { "its fuel ran out" };
    let at = format!("{}:{}:{}", run.file, p.span.line, p.span.col);
    if json {
        eprintln!(
            "{}",
            obj([
                ("ok", false.into()),
                ("paused", true.into()),
                ("steps", Json::from(steps as i64)),
                ("reason", if p.by_time { "time" } else { "fuel" }.into()),
                ("at", at.as_str().into()),
                ("checkpoint", path.into()),
                ("resume", format!("nyra resume {path}").into()),
            ])
        );
    } else {
        eprintln!("nyra: paused after {steps} steps at {at} ({why}); resume with `nyra resume {path}`");
    }
    ExitCode::from(sandbox::PAUSED_EXIT as u8)
}

const USAGE: &str = "\
usage: nyra resume <state.json> [--fuel N] [--max-time MS] [--checkpoint FILE] [--json] [--time]

Goes on with a run that `nyra run --checkpoint state.json` paused, from the statement it stopped
before. What it did before the pause (output, files) is not done again.

  --fuel N           steps for this part (default: the fuel the run was started with)
  --max-time MS      real time for this part (default: the run's)
  --checkpoint FILE  where to save the state if it pauses again (default: the same file)
  --json             errors and the pause notice as JSON

Exit code 125: paused again. A run that ends removes the checkpoint file.
Refused when the program's source, or the program it compiles to, changed since the pause.
";

/// `nyra resume state.json [...]`.
pub fn cli(args: Vec<String>) -> ExitCode {
    let mut file = None;
    let (mut fuel, mut max_time, mut out, mut json, mut time) = (None, None, None, false, false);
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "--fuel" | "--max-time" => {
                let Some(v) = it.next() else { return crate::fail(format!("{a} needs a number")) };
                let Ok(n) = v.replace('_', "").parse::<u64>() else {
                    return crate::fail(format!("{a} needs a whole number, found `{v}`"));
                };
                if a == "--fuel" {
                    fuel = Some(n);
                } else {
                    max_time = Some(n);
                }
            }
            "--checkpoint" => match it.next() {
                Some(p) => out = Some(p),
                None => return crate::fail("--checkpoint needs a file"),
            },
            "--json" => json = true,
            "--time" => time = true,
            _ if a.starts_with('-') => return crate::fail(format!("unknown option `{a}`\n\n{USAGE}")),
            _ if file.is_none() => file = Some(a),
            _ => return crate::fail(format!("`nyra resume` takes one checkpoint file\n\n{USAGE}")),
        }
    }
    let Some(state_path) = file else {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    };
    let text = match std::fs::read_to_string(&state_path) {
        Ok(t) => t,
        Err(e) => return crate::fail(format!("cannot read the checkpoint `{state_path}`: {e}")),
    };
    let mut loaded = match decode(&text) {
        Ok(l) => l,
        Err(e) => return crate::fail(format!("cannot resume from `{state_path}`: {e}")),
    };
    let doc = Json::parse(&text).unwrap_or(Json::Null);
    let src = match std::fs::read_to_string(&loaded.run.file) {
        Ok(s) => s,
        Err(e) => return crate::fail(format!("cannot resume: cannot read the program `{}`: {e}", loaded.run.file)),
    };
    if doc.get("source_hash").and_then(Json::as_str) != Some(hash(&src).as_str()) {
        return crate::fail(format!(
            "cannot resume from `{state_path}`: `{}` changed since the run paused (its state belongs to the old source); run the program again from the start",
            loaded.run.file
        ));
    }
    crate::modules::set_main(Some(Path::new(&loaded.run.file)));
    let mut grant = crate::caps::Grant::none();
    for a in &loaded.run.allow {
        let _ = grant.allow(a);
    }
    let prog = match crate::compile_granted(&src, &grant, &crate::cli_flag) {
        Ok(p) => p,
        Err(diags) => match crate::fix::repair(&src, diags.clone(), |s| crate::compile_granted(s, &grant, &crate::cli_flag)) {
            Some(r) => r.value,
            None => {
                eprint!("{}", crate::diag::render_human(&diags, &loaded.run.file, &src));
                return crate::fail("cannot resume: the program does not compile any more");
            }
        },
    };
    let m = match sandbox::module(&prog) {
        Ok(m) => m,
        Err(msg) => return crate::fail(msg),
    };
    if doc.get("program_hash").and_then(Json::as_str) != Some(program_hash(&m).as_str()) {
        return crate::fail(format!(
            "cannot resume from `{state_path}`: the program compiles differently than when it paused (a file it imports changed, or another version of nyra); run it again from the start"
        ));
    }
    let run = Run { fuel: fuel.unwrap_or(loaded.run.fuel), max_time: max_time.unwrap_or(loaded.run.max_time), ..loaded.run.clone() };
    let mut cfg = sandbox::Config::new();
    cfg.limits.steps = run.fuel;
    cfg.limits.memory = run.max_memory;
    cfg.limits.output = run.max_output;
    cfg.limits.depth = run.max_depth;
    cfg.limits.wall_ms = run.max_time;
    cfg.pausable = true;
    let steps_before = loaded.steps;
    loaded.run = run.clone();
    cfg.resume = Some(loaded);
    let report = sandbox::run(&m, cfg);
    let code = crate::report_interpreted(&report, &run.file, json, time, std::time::Duration::ZERO);
    if report.paused.is_some() {
        let path = out.unwrap_or_else(|| state_path.clone());
        return paused(&m, &run, &src, steps_before, &report, &path, json);
    }
    // over: the same state must not be resumed again (its effects would happen twice)
    let _ = std::fs::remove_file(&state_path);
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs a program to its end in pieces of `fuel` steps: each pause is saved as JSON text, read
    /// back and resumed. Gives the output, the exit code, the steps and how many pauses there were.
    fn in_pieces(m: &Module, stdin: &[u8], args: &[String], fuel: u64) -> (String, i32, u64, usize) {
        let run = Run {
            file: "main.nyra".into(),
            fuel,
            max_memory: sandbox::DEFAULT_MEMORY,
            max_output: sandbox::DEFAULT_OUTPUT,
            max_depth: 100_000,
            max_time: 0,
            allow: vec!["input".into(), "os".into()],
            sandbox: false,
        };
        let (mut out, mut steps, mut pauses) = (String::new(), 0, 0);
        let mut resume: Option<Loaded> = None;
        loop {
            let mut cfg = sandbox::Config::new();
            cfg.limits.steps = fuel;
            cfg.limits.depth = 100_000;
            cfg.to_stdout = false;
            cfg.pausable = true;
            cfg.stdin = Some(stdin.to_vec());
            cfg.args = args.to_vec();
            cfg.resume = resume.take();
            let report = sandbox::run(m, cfg);
            out.push_str(&report.stdout);
            steps += report.steps;
            let Some(p) = &report.paused else {
                assert!(report.internal.is_none(), "{:?}", report.internal);
                return (out, report.exit, steps, pauses);
            };
            pauses += 1;
            let text = encode(m, &run, "source", steps, p).expect("the state is saved");
            resume = Some(decode(&text).unwrap_or_else(|e| panic!("{e}\n{text}")));
            assert!(pauses < 100_000, "no progress");
        }
    }

    fn on_a_big_stack<T: Send>(f: impl FnOnce() -> T + Send) -> T {
        std::thread::scope(|s| std::thread::Builder::new().stack_size(512 << 20).spawn_scoped(s, f).unwrap().join().unwrap())
    }

    /// Every example, paused every few hundred steps and resumed from its JSON each time, prints
    /// what it prints when it runs at once, with the same exit code and the same number of steps.
    #[test]
    fn every_example_paused_and_resumed_gives_the_same_output() {
        let mut paths: Vec<_> = std::fs::read_dir("examples")
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "nyra") && p.with_extension("out").exists())
            .collect();
        paths.sort();
        let (mut checked, mut paused) = (0, 0);
        for path in paths {
            let src = std::fs::read_to_string(&path).unwrap();
            if src.lines().any(|l| l.trim() == "use fs" || l.trim() == "use web") {
                continue;
            }
            let stdin = std::fs::read(path.with_extension("in")).unwrap_or_default();
            let args: Vec<String> = std::fs::read_to_string(path.with_extension("args"))
                .map(|a| a.lines().map(String::from).collect())
                .unwrap_or_default();
            let expected = std::fs::read_to_string(path.with_extension("out")).unwrap().replace("\r\n", "\n");
            paused += on_a_big_stack(|| {
                crate::modules::set_main(Some(path.as_path()));
                let prog = crate::compile(&src).unwrap_or_else(|d| panic!("{}: {d:?}", path.display()));
                let m = sandbox::module(&prog).unwrap();
                // at once
                let mut cfg = sandbox::Config::new();
                cfg.to_stdout = false;
                cfg.limits.depth = 100_000;
                cfg.stdin = Some(stdin.clone());
                cfg.args = args.clone();
                let whole = sandbox::run(&m, cfg);
                assert_eq!(whole.stdout, expected, "{}", path.display());
                // in about 6 pieces (a long benchmark is left to the native tests)
                if whole.steps > 30_000_000 {
                    return 0;
                }
                let fuel = (whole.steps / 6).max(3);
                let (out, exit, steps, pauses) = in_pieces(&m, &stdin, &args, fuel);
                assert_eq!(out, expected, "{} in pieces of {fuel} steps", path.display());
                assert_eq!(exit, whole.exit, "{}", path.display());
                assert_eq!(steps, whole.steps, "{}: the steps of the pieces add up to those of the whole run", path.display());
                pauses
            });
            checked += 1;
        }
        assert!(checked >= 40, "only {checked} examples");
        assert!(paused >= 100, "only {paused} pauses");
    }

    /// Recursion, maps, structs, enums, optionals, tuples, a `for` over a string, values shared
    /// between variables, and a pause after every step.
    #[test]
    fn recursion_and_every_kind_of_value_survive_a_pause_at_every_step() {
        let src = "struct P {\n    name: str\n    tags: [str]\n}\nenum E {\n    A(int, str)\n    B\n}\nfn down(n: int, acc: [int: P]) -> int {\n    if n == 0 {\n        return acc.len()\n    }\n    var m = acc\n    m[n % 7] = P(name: \"p{n}\", tags: [\"t\"])\n    return down(n - 1, m) + 1\n}\nfn main() {\n    let shared = [1.5, -0.0, 3.25]\n    var copy = shared\n    copy.push(2.0)\n    var es: [E] = [E.A(1, \"x\"), E.B]\n    let o: (int, str?) = (4, none)\n    var text = \"\"\n    for c in \"h\u{e9}llo\" {\n        text += \"{c}\"\n    }\n    print(down(40, [:]), shared, copy, es, o, text)\n    for e in es {\n        match e {\n            E.A(n, s) => print(n, s)\n            E.B => print(\"b\")\n        }\n    }\n}\n";
        on_a_big_stack(|| {
            let prog = crate::compile(src).unwrap_or_else(|d| panic!("{d:?}"));
            let m = sandbox::module(&prog).unwrap();
            let mut cfg = sandbox::Config::new();
            cfg.to_stdout = false;
            let whole = sandbox::run(&m, cfg);
            assert!(whole.stdout.starts_with("47 [1.5, 0, 3.25] [1.5, 0, 3.25, 2]"), "{}", whole.stdout);
            for fuel in [1, 2, 3, 17, 100] {
                let (out, exit, steps, pauses) = in_pieces(&m, b"", &[], fuel);
                assert_eq!(out, whole.stdout, "pieces of {fuel}");
                assert_eq!((exit, steps), (0, whole.steps));
                assert!(pauses as u64 >= whole.steps / fuel / 2, "{pauses} pauses with {fuel}");
            }
        });
    }

    #[test]
    fn a_checkpoint_of_another_program_is_refused() {
        let a = "fn main() {\n    var i = 0\n    while i < 100 {\n        i += 1\n    }\n    print(i)\n}\n";
        let b = "struct Q {\n    x: int\n}\nfn other(n: int) -> int = n % 2 + 1\nfn main() {\n    var i = 0\n    while i < 100 {\n        i += other(i)\n    }\n    print(Q(x: i))\n}\n";
        on_a_big_stack(|| {
            let ma = sandbox::module(&crate::compile(a).unwrap()).unwrap();
            let mb = sandbox::module(&crate::compile(b).unwrap()).unwrap();
            assert_ne!(program_hash(&ma), program_hash(&mb));
            let mut cfg = sandbox::Config::new();
            cfg.to_stdout = false;
            cfg.pausable = true;
            cfg.limits.steps = 50;
            let report = sandbox::run(&ma, cfg);
            let run = Run {
                file: "a".into(),
                fuel: 50,
                max_memory: 1,
                max_output: 1,
                max_depth: 1,
                max_time: 0,
                allow: vec![],
                sandbox: true,
            };
            let text = encode(&ma, &run, a, report.steps, report.paused.as_ref().unwrap()).unwrap();
            let loaded = decode(&text).unwrap();
            assert!(loaded.trail(&ma, &[]).is_ok());
            assert!(loaded.trail(&mb, &[]).is_err(), "a frame of another program");
            assert!(decode("{\"x\":1}").err().unwrap().contains("not a checkpoint"));
            assert!(decode("nope").is_err());
        });
    }
}
