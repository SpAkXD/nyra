//! C99 backend. Output is plain, readable C that gcc/clang/tcc can compile.

use std::fmt::Write;

use super::{bare, names};
use crate::ir::{BinOp, Expr, Func, LocalId, Module, PureFn, RtOp, Stmt, StmtKind, Ty, UnOp};

/// The C runtime, emitted before every program (`@FILE@` becomes the source path).
const PRELUDE: &str = include_str!("../rt/c/core.c");
const STRINGS: &str = include_str!("../rt/c/str.c");

const RESERVED: &[&str] = &[
    "auto", "break", "case", "char", "const", "continue", "default", "do", "double", "else", "enum",
    "extern", "float", "for", "goto", "if", "inline", "int", "long", "register", "restrict", "return",
    "short", "signed", "sizeof", "static", "struct", "switch", "typedef", "union", "unsigned", "void",
    "volatile", "while", "bool", "true", "false", "NULL", "EOF", "errno", "stdin", "stdout", "stderr",
    "main", "printf", "puts", "putchar", "fwrite", "snprintf", "sprintf", "strcmp", "strcpy", "strtod", "memcpy",
    "memcmp", "strlen", "atoi", "malloc", "realloc", "free", "exit", "int64_t", "uint32_t", "DBL_MAX", "INT64_MAX",
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
        Ty::Char => "nyrt_char",
        Ty::Str => "nyrt_str*",
        other => unreachable!("the C backend does not handle `{}` yet", other.name()),
    }
}

/// The runtime's name for a type in `nyrt_put_<ty>` / `nyrt_buf_<ty>`.
fn rt_name(t: Ty) -> &'static str {
    match t {
        Ty::Int => "int",
        Ty::Float => "float",
        Ty::Bool => "bool",
        Ty::Char => "char",
        Ty::Str => "str",
        other => unreachable!("cannot print `{}` yet", other.name()),
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
    out.push_str(STRINGS);
    out.push('\n');
    // string literals: read-only objects that are never freed (reference count 0); `const` also
    // lets the C compiler see that releasing one never reaches free()
    for (i, s) in m.strs.iter().enumerate() {
        let _ = writeln!(
            out,
            "static const nyrt_str nyL_{i} = {{0, {}, {}, {}, (char*){}}};",
            s.len(),
            s.len(),
            s.chars().count(),
            string_lit(s)
        );
    }
    if !m.strs.is_empty() {
        out.push('\n');
    }
    let names: Vec<Vec<String>> = m.funcs.iter().map(|f| names::locals(f, var, "nyrt_")).collect();
    for (f, n) in m.funcs.iter().zip(&names) {
        let _ = writeln!(out, "{};", signature(f, n));
    }
    out.push('\n');
    for (f, n) in m.funcs.iter().zip(&names) {
        let _ = writeln!(out, "{} {{", signature(f, n));
        // every local is declared at the top; statements only assign
        for (i, l) in f.locals.iter().enumerate().skip(f.params) {
            let init = if l.ty == Ty::Str { " = NULL" } else { "" };
            let _ = writeln!(out, "    {} {}{init};", ctype(l.ty), n[i]);
        }
        let mut g = Gen { m, f, names: n, out: String::new(), indent: 1, tmp: 0 };
        g.stmts(&f.body);
        out.push_str(&g.out);
        out.push_str("}\n\n");
    }
    let _ = write!(
        out,
        "int main(void) {{\n    nyrt_init();\n    ny_{}();\n    nyrt_leak_check();\n    return 0;\n}}\n",
        m.func(m.main).name
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
                let line = self.assign(*dst, format!("ny_{}({})", self.m.func(*func).name, args.join(", ")));
                self.line(&line);
            }
            StmtKind::Op { dst, op, args } => self.op(*dst, *op, args, &at),
            StmtKind::If { .. } => self.if_chain(s),
            StmtKind::Loop { head, cond, body, step } => self.lp(head, cond, body, step),
            StmtKind::ForEach { var, iter, body } => {
                self.tmp += 1;
                let (p, adv) = (format!("nyrt_p{}", self.tmp), format!("nyrt_adv{}", self.tmp));
                let s = self.expr(iter);
                let v = self.local(*var).to_string();
                self.line(&format!("for (int64_t {p} = 0; {p} < {s}->len; ) {{"));
                self.indent += 1;
                self.line(&format!("int64_t {adv};"));
                self.line(&format!("{v} = nyrt_utf8_decode({s}->data + {p}, &{adv});"));
                self.line(&format!("{p} += {adv};"));
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
            StmtKind::Dup(l) => {
                let line = format!("nyrt_str_retain({});", self.local(*l));
                self.line(&line);
            }
            StmtKind::Drop(l) => {
                let line = format!("nyrt_str_release({});", self.local(*l));
                self.line(&line);
            }
            StmtKind::Free(l) => {
                let x = self.local(*l).to_string();
                self.line(&format!("nyrt_str_release({x});"));
                self.line(&format!("{x} = NULL;"));
            }
        }
    }

    fn op(&mut self, dst: Option<LocalId>, op: RtOp, args: &[Expr], at: &str) {
        let a: Vec<String> = args.iter().map(|x| self.arg(x)).collect();
        let call = match op {
            RtOp::Print => {
                // each part straight to stdout: no temporary string
                for p in args {
                    let line = self.put("nyrt_put", p);
                    self.line(&line);
                }
                self.line("putchar('\\n');");
                return;
            }
            RtOp::Format => {
                let d = dst.map(|d| self.local(d).to_string()).unwrap_or_default();
                self.line("{");
                self.indent += 1;
                self.line("nyrt_buf b = nyrt_buf_new();");
                for p in args {
                    let line = self.put("nyrt_buf", p);
                    self.line(&line);
                }
                self.line(&format!("{d} = nyrt_buf_done(&b);"));
                self.indent -= 1;
                self.line("}");
                return;
            }
            RtOp::StrAppend => {
                let d = dst.map(|d| self.local(d).to_string()).unwrap_or_default();
                self.line(&format!("nyrt_str_append(&{d}, {});", a[1]));
                return;
            }
            RtOp::DivInt => format!("nyrt_div({}, {}, {at})", a[0], a[1]),
            RtOp::RemInt => format!("nyrt_mod({}, {}, {at})", a[0], a[1]),
            RtOp::FloatToInt => format!("nyrt_f2i({}, {at})", a[0]),
            RtOp::StrConcat => format!("nyrt_str_concat({}, {})", a[0], a[1]),
            RtOp::StrAt => format!("nyrt_str_at({}, {}, {at})", a[0], a[1]),
            RtOp::StrSlice => format!("nyrt_str_slice({}, {}, {}, {at})", a[0], a[1], a[2]),
            RtOp::StrReplace => format!("nyrt_str_replace({}, {}, {}, {at})", a[0], a[1], a[2]),
            RtOp::StrTrim => format!("nyrt_str_trim({})", a[0]),
            RtOp::StrUpper => format!("nyrt_str_upper({})", a[0]),
            RtOp::StrLower => format!("nyrt_str_lower({})", a[0]),
            RtOp::StrRepeat => format!("nyrt_str_repeat({}, {}, {at})", a[0], a[1]),
            RtOp::StrToInt => format!("nyrt_str_to_int({}, {at})", a[0]),
            RtOp::StrToFloat => format!("nyrt_str_to_float({}, {at})", a[0]),
            RtOp::CharFrom => format!("nyrt_char_from({}, {at})", a[0]),
        };
        let line = self.assign(dst, call);
        self.line(&line);
    }

    /// `nyrt_put_int(x);` / `nyrt_buf_int(&b, x);` for one part of a print or format.
    fn put(&self, prefix: &str, p: &Expr) -> String {
        let target = if prefix == "nyrt_buf" { "&b, " } else { "" };
        if let Expr::Str(id) = p {
            let s = self.m.str(*id);
            return format!("{prefix}_lit({target}{}, {});", string_lit(s), s.len());
        }
        format!("{prefix}_{}({target}{});", rt_name(p.ty(self.f)), bare(&self.expr(p)))
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
            Expr::Char(c) => c.to_string(),
            Expr::Str(id) => format!("((nyrt_str*)&nyL_{})", id.0),
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
                    BinOp::SEq => format!("nyrt_str_eq({}, {})", bare(&a), bare(&b)),
                    BinOp::SNe => format!("(!nyrt_str_eq({}, {}))", bare(&a), bare(&b)),
                    BinOp::SLt | BinOp::SLe | BinOp::SGt | BinOp::SGe => {
                        format!("(nyrt_str_cmp({}, {}) {} 0)", bare(&a), bare(&b), op.symbol())
                    }
                    _ => format!("({a} {} {b})", op.symbol()),
                }
            }
            Expr::Select(c, a, b) => format!("({} ? {} : {})", self.expr(c), self.expr(a), self.expr(b)),
            Expr::IntToFloat(x) => format!("((double)({}))", self.expr(x)),
            Expr::Pure(p, args) => {
                let a: Vec<String> = args.iter().map(|x| self.arg(x)).collect();
                match p {
                    PureFn::StrLen => format!("{}->nchars", self.expr(&args[0])),
                    PureFn::StrContains => format!("nyrt_str_contains({}, {})", a[0], a[1]),
                    PureFn::StrStartsWith => format!("nyrt_str_starts_with({}, {})", a[0], a[1]),
                    PureFn::StrEndsWith => format!("nyrt_str_ends_with({}, {})", a[0], a[1]),
                    PureFn::StrIndexOf => format!("nyrt_str_index_of({}, {})", a[0], a[1]),
                    PureFn::CharCode => format!("((int64_t)({}))", a[0]),
                    PureFn::CharUpper => format!("nyrt_char_upper({})", a[0]),
                    PureFn::CharLower => format!("nyrt_char_lower({})", a[0]),
                    PureFn::CharIsDigit => format!("nyrt_char_is_digit({})", a[0]),
                    PureFn::CharIsLetter => format!("nyrt_char_is_letter({})", a[0]),
                    PureFn::CharIsUpper => format!("nyrt_char_is_upper({})", a[0]),
                    PureFn::CharIsLower => format!("nyrt_char_is_lower({})", a[0]),
                    PureFn::CharIsSpace => format!("nyrt_is_space({})", a[0]),
                }
            }
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
