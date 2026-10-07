//! C99 backend. Output is plain, readable C that gcc/clang/tcc can compile.

use std::fmt::Write;

use super::{bare, names};
use crate::ir::{BinOp, Expr, Func, LocalId, Module, RtOp, Stmt, StmtKind, Ty, UnOp};

/// The C runtime, emitted before every program (`@FILE@` becomes the source path).
const PRELUDE: &str = include_str!("../rt/c/core.c");

const RESERVED: &[&str] = &[
    "auto", "break", "case", "char", "const", "continue", "default", "do", "double", "else", "enum",
    "extern", "float", "for", "goto", "if", "inline", "int", "long", "register", "restrict", "return",
    "short", "signed", "sizeof", "static", "struct", "switch", "typedef", "union", "unsigned", "void",
    "volatile", "while", "bool", "true", "false", "NULL", "EOF", "errno", "stdin", "stdout", "stderr",
    "main", "printf", "puts", "snprintf", "sprintf", "strcmp", "strcpy", "strtod", "memcpy", "atoi", "malloc",
    "exit", "int64_t", "DBL_MAX",
];

fn var(name: &str) -> String {
    // `ny...` names belong to nyra (user functions are `ny_<name>`, the runtime is `nyrt_*`)
    if RESERVED.contains(&name) || name.starts_with("ny") {
        format!("{name}_")
    } else {
        name.to_string()
    }
}

fn ctype(t: Ty) -> &'static str {
    match t {
        Ty::Int => "int64_t",
        Ty::Float => "double",
        Ty::Bool => "bool",
        Ty::Str => "const char*",
    }
}

fn signature(f: &Func, names: &[String]) -> String {
    let params = if f.params == 0 {
        "void".to_string()
    } else {
        (0..f.params).map(|i| format!("{} {}", ctype(f.locals[i].ty), names[i])).collect::<Vec<_>>().join(", ")
    };
    format!("static {} ny_{}({params})", f.ret.map_or("void", ctype), f.name)
}

/// `file` is the source path as given to nyra; runtime errors report it.
pub fn gen(m: &Module, file: &str) -> String {
    let mut out = PRELUDE.replace("@FILE@", &string_lit(file));
    let names: Vec<Vec<String>> = m.funcs.iter().map(|f| names::locals(f, var, "nyrt_")).collect();
    for (f, n) in m.funcs.iter().zip(&names) {
        let _ = writeln!(out, "{};", signature(f, n));
    }
    out.push('\n');
    for (f, n) in m.funcs.iter().zip(&names) {
        let _ = writeln!(out, "{} {{", signature(f, n));
        // every local is declared at the top; statements only assign
        for (i, l) in f.locals.iter().enumerate().skip(f.params) {
            let _ = writeln!(out, "    {} {};", ctype(l.ty), n[i]);
        }
        let mut g = Gen { m, f, names: n, out: String::new(), indent: 1 };
        g.stmts(&f.body);
        out.push_str(&g.out);
        out.push_str("}\n\n");
    }
    let _ = write!(out, "int main(void) {{\n    ny_{}();\n    return 0;\n}}\n", m.func(m.main).name);
    out
}

struct Gen<'a> {
    m: &'a Module,
    f: &'a Func,
    names: &'a [String],
    out: String,
    indent: usize,
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

    fn stmt(&mut self, s: &Stmt) {
        let at = format!("{}, {}", s.span.line, s.span.col);
        match &s.kind {
            StmtKind::Set(l, e) => {
                let line = format!("{} = {};", self.local(*l), bare(&self.expr(e)));
                self.line(&line);
            }
            StmtKind::Call { dst, func, args } => {
                let args: Vec<String> = args.iter().map(|a| bare(&self.expr(a)).to_string()).collect();
                let line = self.assign(*dst, format!("ny_{}({})", self.m.func(*func).name, args.join(", ")));
                self.line(&line);
            }
            StmtKind::Op { dst, op, args } => {
                let code = match op {
                    RtOp::Print => self.print(args),
                    RtOp::Format => {
                        let (fmt, a) = self.format(args);
                        format!("nyrt_fmt({}{a})", string_lit(&fmt))
                    }
                    RtOp::DivInt => format!("nyrt_div({}, {}, {at})", self.expr(&args[0]), self.expr(&args[1])),
                    RtOp::RemInt => format!("nyrt_mod({}, {}, {at})", self.expr(&args[0]), self.expr(&args[1])),
                    RtOp::FloatToInt => format!("nyrt_f2i({}, {at})", self.expr(&args[0])),
                };
                let line = self.assign(*dst, code);
                self.line(&line);
            }
            StmtKind::If { .. } => self.if_chain(s),
            StmtKind::Loop { head, cond, body, step } => self.lp(head, cond, body, step),
            StmtKind::Return(None) => self.line("return;"),
            StmtKind::Return(Some(e)) => {
                let line = format!("return {};", bare(&self.expr(e)));
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
        let c = bare(&full);
        match (head, step) {
            ([], []) => {
                self.line(&format!("while ({c}) {{"));
                self.block(body);
            }
            ([], [Stmt { kind: StmtKind::Set(l, e), .. }]) => {
                let line = format!("for (; {c}; {} = {}) {{", self.local(*l), bare(&self.expr(e)));
                self.line(&line);
                self.block(body);
            }
            _ => {
                self.line("for (;;) {");
                self.indent += 1;
                self.stmts(head);
                self.line(&format!("if (!{full}) break;"));
                self.stmts(body);
                self.stmts(step);
                self.indent -= 1;
            }
        }
        self.line("}");
    }

    /// One part uses the matching print helper; several parts become one `printf`.
    fn print(&self, parts: &[Expr]) -> String {
        if let [p] = parts {
            return format!("nyrt_print_{}({})", p.ty(self.f).name(), self.expr(p));
        }
        let (fmt, a) = self.format(parts);
        format!("printf({}{a})", string_lit(&(fmt + "\n")))
    }

    /// A printf-style format string and its argument list (", a, b") for string parts.
    fn format(&self, parts: &[Expr]) -> (String, String) {
        let mut fmt = String::new();
        let mut args = String::new();
        for p in parts {
            if let Expr::Str(id) = p {
                fmt.push_str(&self.m.str(*id).replace('%', "%%"));
                continue;
            }
            let x = self.expr(p);
            let (spec, arg) = match p.ty(self.f) {
                Ty::Int => ("%lld", format!("(long long)({x})")),
                Ty::Float => ("%s", format!("nyrt_float_fmt((char[32]){{0}}, {x})")),
                Ty::Bool => ("%s", format!("(({x}) ? \"true\" : \"false\")")),
                Ty::Str => ("%s", x),
            };
            fmt.push_str(spec);
            args.push_str(", ");
            args.push_str(&arg);
        }
        (fmt, args)
    }

    fn expr(&self, e: &Expr) -> String {
        match e {
            Expr::Int(i64::MIN) => "INT64_MIN".to_string(),
            Expr::Int(n) => {
                if *n > i32::MAX as i64 || *n < i32::MIN as i64 {
                    format!("INT64_C({n})")
                } else {
                    n.to_string()
                }
            }
            Expr::Float(f) => format!("{f:?}"),
            Expr::Bool(b) => b.to_string(),
            Expr::Str(id) => string_lit(self.m.str(*id)),
            Expr::Local(l) => self.local(*l).to_string(),
            Expr::Unary(op, x) => {
                let x = self.expr(x);
                match op {
                    UnOp::Not => format!("(!{x})"),
                    _ if x.starts_with('-') => format!("(-({x}))"),
                    _ => format!("(-{x})"),
                }
            }
            Expr::Binary(op, a, b) => {
                let (a, b) = (self.expr(a), self.expr(b));
                match op {
                    BinOp::SEq => format!("(strcmp({a}, {b}) == 0)"),
                    BinOp::SNe => format!("(strcmp({a}, {b}) != 0)"),
                    _ => format!("({a} {} {b})", op.symbol()),
                }
            }
            Expr::Select(c, a, b) => format!("({} ? {} : {})", self.expr(c), self.expr(a), self.expr(b)),
            Expr::IntToFloat(x) => format!("((double)({}))", self.expr(x)),
        }
    }
}

fn string_lit(s: &str) -> String {
    let mut out = String::from("\"");
    for b in s.bytes() {
        match b {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            b'\t' => out.push_str("\\t"),
            b'\r' => out.push_str("\\r"),
            b'?' => out.push_str("\\?"),
            0x20..=0x7e => out.push(b as char),
            _ => {
                let _ = write!(out, "\\{b:03o}");
            }
        }
    }
    out.push('"');
    out
}
