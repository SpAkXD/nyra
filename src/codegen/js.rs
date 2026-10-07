//! JavaScript backend. Output runs on Node.js or in the browser.

use std::fmt::Write;

use super::{bare, names};
use crate::ir::{BinOp, Expr, Func, LocalId, Module, PureFn, RtOp, Stmt, StmtKind, Ty, UnOp};

const RESERVED: &[&str] = &[
    "arguments", "await", "break", "case", "catch", "class", "const", "continue", "debugger", "default",
    "delete", "do", "else", "enum", "eval", "export", "extends", "false", "finally", "for", "function",
    "if", "implements", "import", "in", "instanceof", "interface", "let", "new", "null", "package",
    "private", "protected", "public", "return", "static", "super", "switch", "this", "throw", "true",
    "try", "typeof", "var", "void", "while", "with", "yield", "undefined", "NaN", "Infinity", "console",
    "Math", "String", "Number", "Object", "Array", "JSON", "Symbol", "BigInt", "Error", "globalThis",
    "process", "require", "module", "exports", "NyPanic", "NY_SURR",
];

/// The JavaScript runtime, emitted before every program (`@FILE@` becomes the source path).
const PRELUDE: &str = include_str!("../rt/js/core.js");
const STRINGS: &str = include_str!("../rt/js/str.js");

fn name(n: &str) -> String {
    if RESERVED.contains(&n) || n.starts_with("ny_") {
        format!("{n}_")
    } else {
        n.to_string()
    }
}

/// `file` is the source path as given to nyra; runtime errors report it.
pub fn gen(m: &Module, file: &str) -> String {
    let mut out = PRELUDE.replace("@FILE@", &crate::diag::json_str(file));
    out.push('\n');
    out.push_str(STRINGS);
    out.push('\n');
    for f in &m.funcs {
        let n = names::locals(f, name, "ny_");
        let _ = writeln!(out, "function {}({}) {{", name(&f.name), n[..f.params].join(", "));
        // every local is declared at the top; statements only assign
        if f.locals.len() > f.params {
            let _ = writeln!(out, "    let {};", n[f.params..].join(", "));
        }
        let mut g = Gen { m, f, names: &n, out: String::new(), indent: 1, tmp: 0 };
        g.stmts(&f.body);
        out.push_str(&g.out);
        out.push_str("}\n\n");
    }
    let _ = write!(
        out,
        concat!(
            "try {{\n",
            "    {}();\n",
            "}} catch (e) {{\n",
            "    console.error(ny_rescue(e).message);\n",
            "    // exitCode instead of process.exit(), so buffered stdout is never cut off\n",
            "    if (typeof process !== \"undefined\") process.exitCode = 101;\n",
            "}}\n",
        ),
        name(&m.func(m.main).name)
    );
    out
}

struct Gen<'a> {
    m: &'a Module,
    f: &'a Func,
    names: &'a [String],
    out: String,
    indent: usize,
    /// Counter for the helper names of `for` loops over strings.
    tmp: usize,
}

impl Gen<'_> {
    fn line(&mut self, s: &str) {
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn block(&mut self, ss: &[Stmt]) {
        self.indent += 1;
        self.stmts(ss);
        self.indent -= 1;
    }

    fn stmts(&mut self, ss: &[Stmt]) {
        for s in ss {
            self.stmt(s);
        }
    }

    fn local(&self, l: LocalId) -> &str {
        &self.names[l.0 as usize]
    }

    /// `dst = value;` or `value;`
    fn assign(&self, dst: Option<LocalId>, value: String) -> String {
        match dst {
            Some(d) => format!("{} = {value};", self.local(d)),
            None => format!("{value};"),
        }
    }

    fn arg(&self, e: &Expr) -> String {
        bare(&self.expr(e)).to_string()
    }

    fn stmt(&mut self, s: &Stmt) {
        let at = format!("{}, {}", s.span.line, s.span.col);
        match &s.kind {
            StmtKind::Set(l, e) => {
                let line = format!("{} = {};", self.local(*l), bare(&self.expr(e)));
                self.line(&line);
            }
            StmtKind::Call { dst, func, args } => {
                let args: Vec<String> = args.iter().map(|a| self.arg(a)).collect();
                let line = self.assign(*dst, format!("{}({})", name(&self.m.func(*func).name), args.join(", ")));
                self.line(&line);
            }
            StmtKind::Op { dst, op, args } => {
                let a: Vec<String> = args.iter().map(|x| self.arg(x)).collect();
                let code = match op {
                    RtOp::Print => self.print(args),
                    RtOp::Format => self.template(args),
                    RtOp::StrAppend => {
                        // the destination is always the first operand (the verifier checks it)
                        let line = format!("{} += {};", a[0], self.expr(&args[1]));
                        self.line(&line);
                        return;
                    }
                    RtOp::DivInt => format!("ny_div({}, {}, {at})", a[0], a[1]),
                    RtOp::RemInt => format!("ny_mod({}, {}, {at})", a[0], a[1]),
                    RtOp::FloatToInt => format!("ny_f2i({}, {at})", a[0]),
                    RtOp::StrConcat => format!("{} + {}", self.expr(&args[0]), self.expr(&args[1])),
                    RtOp::StrAt => format!("ny_str_at({}, {}, {at})", a[0], a[1]),
                    RtOp::StrSlice => format!("ny_str_slice({}, {}, {}, {at})", a[0], a[1], a[2]),
                    RtOp::StrReplace => format!("ny_str_replace({}, {}, {}, {at})", a[0], a[1], a[2]),
                    RtOp::StrTrim => format!("ny_str_trim({})", a[0]),
                    RtOp::StrUpper => format!("ny_str_upper({})", a[0]),
                    RtOp::StrLower => format!("ny_str_lower({})", a[0]),
                    RtOp::StrRepeat => format!("ny_str_repeat({}, {}, {at})", a[0], a[1]),
                    RtOp::StrToInt => format!("ny_str_to_int({}, {at})", a[0]),
                    RtOp::StrToFloat => format!("ny_str_to_float({}, {at})", a[0]),
                    RtOp::CharFrom => format!("ny_char_from({}, {at})", a[0]),
                };
                let line = self.assign(*dst, code);
                self.line(&line);
            }
            StmtKind::If { .. } => self.if_chain(s),
            StmtKind::Loop { head, cond, body, step } => self.lp(head, cond, body, step),
            StmtKind::ForEach { var, iter, body } => {
                // `for...of` walks a string by code points, like Nyra
                self.tmp += 1;
                let c = format!("ny_c{}", self.tmp);
                let line = format!("for (const {c} of {}) {{", self.arg(iter));
                self.line(&line);
                self.indent += 1;
                let line = format!("{} = {c}.codePointAt(0);", self.local(*var));
                self.line(&line);
                self.stmts(body);
                self.indent -= 1;
                self.line("}");
            }
            StmtKind::Break => self.line("break;"),
            StmtKind::Continue => self.line("continue;"),
            StmtKind::Return(None) => self.line("return;"),
            StmtKind::Return(Some(e)) => {
                let line = format!("return {};", bare(&self.expr(e)));
                self.line(&line);
            }
            // the garbage collector owns JavaScript memory
            StmtKind::Dup(_) | StmtKind::Drop(_) => {}
            StmtKind::Free(l) => {
                let line = format!("{} = undefined;", self.local(*l));
                self.line(&line);
            }
        }
    }

    fn if_chain(&mut self, s: &Stmt) {
        let mut cur = s;
        let mut head = "if";
        loop {
            let StmtKind::If { cond, then, els } = &cur.kind else { unreachable!() };
            let line = format!("{head} ({}) {{", bare(&self.expr(cond)));
            self.line(&line);
            self.block(then);
            match els.as_slice() {
                [] => break,
                [next] if matches!(next.kind, StmtKind::If { .. }) => {
                    cur = next;
                    head = "} else if";
                }
                _ => {
                    self.line("} else {");
                    self.block(els);
                    break;
                }
            }
        }
        self.line("}");
    }

    fn lp(&mut self, head: &[Stmt], cond: &Expr, body: &[Stmt], step: &[Stmt]) {
        let full = self.expr(cond);
        let c = bare(&full).to_string();
        // the step goes in the `for` header, so `continue` runs it
        let steps: Vec<String> = step
            .iter()
            .map(|s| match &s.kind {
                StmtKind::Set(l, e) => format!("{} = {}", self.local(*l), bare(&self.expr(e))),
                _ => unreachable!("a loop step only assigns"),
            })
            .collect();
        let step = steps.join(", ");
        match (head.is_empty(), step.is_empty()) {
            (true, true) => {
                self.line(&format!("while ({c}) {{"));
                self.block(body);
            }
            (true, false) => {
                self.line(&format!("for (; {c}; {step}) {{"));
                self.block(body);
            }
            (false, _) => {
                self.line(&format!("for (;; {step}) {{"));
                self.indent += 1;
                self.stmts(head);
                self.line(&format!("if (!{full}) break;"));
                self.stmts(body);
                self.indent -= 1;
            }
        }
        self.line("}");
    }

    /// Numbers print through `String()` so `-0` shows as `0`, like the C backend.
    fn print(&self, parts: &[Expr]) -> String {
        if let [p] = parts {
            let x = self.arg(p);
            return match p.ty(self.f) {
                Ty::Int | Ty::Float => format!("console.log(String({x}))"),
                Ty::Char => format!("console.log(ny_char_str({x}))"),
                _ => format!("console.log({x})"),
            };
        }
        format!("console.log({})", self.template(parts))
    }

    /// A template literal for string parts: text is escaped, values become `${...}`.
    fn template(&self, parts: &[Expr]) -> String {
        let mut s = String::from("`");
        for p in parts {
            if let Expr::Str(id) = p {
                for c in self.m.str(*id).chars() {
                    match c {
                        '\\' => s.push_str("\\\\"),
                        '`' => s.push_str("\\`"),
                        '$' => s.push_str("\\$"),
                        '\n' => s.push_str("\\n"),
                        '\r' => s.push_str("\\r"),
                        '\t' => s.push_str("\\t"),
                        c if (c as u32) < 0x20 => s.push_str(&format!("\\u{:04x}", c as u32)),
                        c => s.push(c),
                    }
                }
                continue;
            }
            s.push_str("${");
            if p.ty(self.f) == Ty::Char {
                let _ = write!(s, "ny_char_str({})", self.arg(p));
            } else {
                s.push_str(&self.arg(p));
            }
            s.push('}');
        }
        s.push('`');
        s
    }

    fn expr(&self, e: &Expr) -> String {
        match e {
            Expr::Int(n) => n.to_string(),
            Expr::Float(f) => format!("{f:?}"),
            Expr::Bool(b) => b.to_string(),
            Expr::Char(c) => c.to_string(),
            Expr::Str(id) => crate::diag::json_str(self.m.str(*id)),
            Expr::Local(l) => self.local(*l).to_string(),
            Expr::Unary(op, x) => {
                let x = self.expr(x);
                // never print `--x` (that would be a decrement)
                let neg = if x.starts_with('-') { format!("-({x})") } else { format!("-{x}") };
                match op {
                    // `+ 0` turns an int -0 into 0 (C has no -0 for ints)
                    UnOp::INeg => format!("({neg} + 0)"),
                    UnOp::FNeg => format!("({neg})"),
                    UnOp::Not => format!("(!{x})"),
                }
            }
            Expr::Binary(op, a, b) => {
                let (a, b) = (self.expr(a), self.expr(b));
                match op {
                    BinOp::IMul => format!("({a} * {b} + 0)"),
                    BinOp::IDiv => format!("(Math.trunc({a} / {b}) + 0)"),
                    BinOp::IRem => format!("({a} % {b} + 0)"),
                    BinOp::IEq | BinOp::FEq | BinOp::BEq | BinOp::CEq | BinOp::SEq => format!("({a} === {b})"),
                    BinOp::INe | BinOp::FNe | BinOp::BNe | BinOp::CNe | BinOp::SNe => format!("({a} !== {b})"),
                    // code point order (JavaScript's `<` compares UTF-16 units)
                    BinOp::SLt | BinOp::SLe | BinOp::SGt | BinOp::SGe => {
                        format!("(ny_str_cmp({}, {}) {} 0)", bare(&a), bare(&b), op.symbol())
                    }
                    _ => format!("({a} {} {b})", op.symbol()),
                }
            }
            Expr::Select(c, a, b) => format!("({} ? {} : {})", self.expr(c), self.expr(a), self.expr(b)),
            Expr::IntToFloat(x) => self.expr(x),
            Expr::Pure(p, args) => {
                let a: Vec<String> = args.iter().map(|x| self.arg(x)).collect();
                match p {
                    PureFn::StrLen => format!("ny_len({})", a[0]),
                    PureFn::StrContains => format!("{}.includes({})", self.expr(&args[0]), a[1]),
                    PureFn::StrStartsWith => format!("{}.startsWith({})", self.expr(&args[0]), a[1]),
                    PureFn::StrEndsWith => format!("{}.endsWith({})", self.expr(&args[0]), a[1]),
                    PureFn::StrIndexOf => format!("ny_str_index_of({}, {})", a[0], a[1]),
                    PureFn::CharCode => self.expr(&args[0]),
                    PureFn::CharUpper => format!("ny_char_upper({})", a[0]),
                    PureFn::CharLower => format!("ny_char_lower({})", a[0]),
                    PureFn::CharIsDigit => format!("ny_char_is_digit({})", a[0]),
                    PureFn::CharIsLetter => format!("ny_char_is_letter({})", a[0]),
                    PureFn::CharIsUpper => format!("ny_char_is_upper({})", a[0]),
                    PureFn::CharIsLower => format!("ny_char_is_lower({})", a[0]),
                    PureFn::CharIsSpace => format!("ny_is_space({})", a[0]),
                }
            }
        }
    }
}
