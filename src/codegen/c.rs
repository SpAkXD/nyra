//! C99 backend. Output is plain, readable C that gcc/clang/tcc can compile.

use std::fmt::Write;

use super::{bare, names};
use crate::ir::{BinOp, Expr, Func, LocalId, Module, Place, PureFn, RtOp, Step, Stmt, StmtKind, Ty, UnOp};

/// The C runtime, emitted before every program (`@FILE@` becomes the source path).
const PRELUDE: &str = include_str!("../rt/c/core.c");
const STRINGS: &str = include_str!("../rt/c/str.c");
const ARRAYS: &str = include_str!("../rt/c/arr.c");

const RESERVED: &[&str] = &[
    "auto", "break", "case", "char", "const", "continue", "default", "do", "double", "else", "enum",
    "extern", "float", "for", "goto", "if", "inline", "int", "long", "register", "restrict", "return",
    "short", "signed", "sizeof", "static", "struct", "switch", "typedef", "union", "unsigned", "void",
    "volatile", "while", "bool", "true", "false", "NULL", "EOF", "errno", "stdin", "stdout", "stderr",
    "main", "printf", "puts", "putchar", "fwrite", "fputs", "snprintf", "sprintf", "strcmp", "strcpy", "strtod",
    "memcpy", "memmove", "memcmp", "strlen", "atoi", "malloc", "realloc", "free", "exit", "getenv", "int64_t",
    "uint32_t", "uint64_t", "size_t", "DBL_MAX", "INT64_MAX", "INT64_MIN",
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
        Ty::Array(_) => "nyrt_arr*",
        other => unreachable!("the C backend does not handle `{}` yet", other.name()),
    }
}

/// The runtime's name for a type in `nyrt_put_<ty>` / `nyrt_buf_<ty>` / `nyrt_<ty>_retain`.
fn rt_name(t: Ty) -> &'static str {
    match t {
        Ty::Int => "int",
        Ty::Float => "float",
        Ty::Bool => "bool",
        Ty::Char => "char",
        Ty::Str => "str",
        Ty::Array(_) => "arr",
        other => unreachable!("no runtime functions for `{}` yet", other.name()),
    }
}

/// The element type descriptor of an array of `elem`.
fn desc(elem: Ty) -> String {
    format!("&nyrt_T_{}", rt_name(elem))
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
    out.push_str(ARRAYS);
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
            let init = if m.managed(l.ty) { " = NULL" } else { "" };
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
    /// Counter for helper names (loop cursors, element pointers).
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

    fn ty(&self, e: &Expr) -> Ty {
        e.ty(self.f)
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

    /// The address of a value of type `t`: `&x` for a local, a compound literal otherwise.
    fn addr(&self, e: &Expr, t: Ty) -> String {
        match e {
            Expr::Local(l) => format!("&{}", self.local(*l)),
            _ => format!("&({}){{{}}}", ctype(t), self.arg(e)),
        }
    }

    fn fresh(&mut self, prefix: &str) -> String {
        self.tmp += 1;
        format!("nyrt_{prefix}{}", self.tmp)
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
            StmtKind::Store { place, value } => {
                self.line("{");
                self.indent += 1;
                let (parent, last) = self.place_parent(place);
                let t = self.ty(value);
                let slot = match last {
                    Some((i, span)) => {
                        let p = self.fresh("p");
                        let line = format!(
                            "{} *{p} = ({}*)nyrt_arr_at({parent}, {}, {}, {});",
                            ctype(t),
                            ctype(t),
                            self.arg(&i),
                            span.line,
                            span.col
                        );
                        self.line(&line);
                        format!("(*{p})")
                    }
                    None => parent,
                };
                let v = self.arg(value);
                if self.m.managed(t) {
                    // the new value gets its owner before the old one loses its own (`xs[0] = xs[0]`)
                    let n = rt_name(t);
                    self.line(&format!("nyrt_{n}_retain({v});"));
                    self.line(&format!("nyrt_{n}_release({slot});"));
                }
                self.line(&format!("{slot} = {v};"));
                self.indent -= 1;
                self.line("}");
            }
            StmtKind::Mutate { dst, op, place, args } => self.mutate(*dst, *op, place, args, &at),
            StmtKind::If { .. } => self.if_chain(s),
            StmtKind::Loop { head, cond, body, step } => self.lp(head, cond, body, step),
            StmtKind::ForEach { var, iter, body } => {
                let it = self.expr(iter);
                let v = self.local(*var).to_string();
                match self.ty(iter) {
                    Ty::Str => {
                        let (p, adv) = (self.fresh("p"), self.fresh("adv"));
                        self.line(&format!("for (int64_t {p} = 0; {p} < {it}->len; ) {{"));
                        self.indent += 1;
                        self.line(&format!("int64_t {adv};"));
                        self.line(&format!("{v} = nyrt_utf8_decode({it}->data + {p}, &{adv});"));
                        self.line(&format!("{p} += {adv};"));
                    }
                    t => {
                        let elem = t.elem().expect("verified: an array");
                        let i = self.fresh("i");
                        self.line(&format!("for (int64_t {i} = 0; {i} < {it}->len; {i}++) {{"));
                        self.indent += 1;
                        self.line(&format!("{v} = (({}*){it}->data)[{i}];", ctype(elem)));
                    }
                }
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
                let line = format!("nyrt_{}_retain({});", rt_name(self.f.local(*l).ty), self.local(*l));
                self.line(&line);
            }
            StmtKind::Drop(l) => {
                let line = format!("nyrt_{}_release({});", rt_name(self.f.local(*l).ty), self.local(*l));
                self.line(&line);
            }
            StmtKind::Free(l) => {
                let x = self.local(*l).to_string();
                let n = rt_name(self.f.local(*l).ty);
                self.line(&format!("nyrt_{n}_release({x});"));
                self.line(&format!("{x} = NULL;"));
            }
        }
    }

    /// Emits what makes every array above the last step of `p` unique and checks its indexes.
    /// Returns a C lvalue for the value that holds the last step, and the last index (`None`
    /// when the place is the whole local).
    fn place_parent(&mut self, p: &Place) -> (String, Option<(Expr, crate::ast::Span)>) {
        let mut lv = self.local(p.root).to_string();
        let mut t = self.f.local(p.root).ty;
        let n = p.path.len();
        for (k, step) in p.path.iter().enumerate() {
            match step {
                Step::Index(i, span) => {
                    // this array is about to change below this point
                    self.line(&format!("nyrt_arr_unique(&{lv});"));
                    if k + 1 == n {
                        return (lv, Some((i.clone(), *span)));
                    }
                    let elem = t.elem().expect("verified: an array");
                    let ptr = self.fresh("p");
                    let line = format!(
                        "{} *{ptr} = ({}*)nyrt_arr_at({lv}, {}, {}, {});",
                        ctype(elem),
                        ctype(elem),
                        self.arg(i),
                        span.line,
                        span.col
                    );
                    self.line(&line);
                    lv = format!("(*{ptr})");
                    t = elem;
                }
                Step::Field(_) => unreachable!("structs are not lowered yet"),
            }
        }
        (lv, None)
    }

    /// An lvalue for the whole place (the last element too), every array on the way unique.
    fn place(&mut self, p: &Place) -> String {
        let (parent, last) = self.place_parent(p);
        match last {
            None => parent,
            Some((i, span)) => {
                let t = self.place_ty(p);
                let ptr = self.fresh("p");
                let line = format!(
                    "{} *{ptr} = ({}*)nyrt_arr_at({parent}, {}, {}, {});",
                    ctype(t),
                    ctype(t),
                    self.arg(&i),
                    span.line,
                    span.col
                );
                self.line(&line);
                format!("(*{ptr})")
            }
        }
    }

    fn place_ty(&self, p: &Place) -> Ty {
        let mut t = self.f.local(p.root).ty;
        for s in &p.path {
            t = match s {
                Step::Index(..) => t.elem().expect("verified: an array"),
                Step::Field(_) => unreachable!("structs are not lowered yet"),
            };
        }
        t
    }

    fn mutate(&mut self, dst: Option<LocalId>, op: RtOp, place: &Place, args: &[Expr], at: &str) {
        self.line("{");
        self.indent += 1;
        let lv = self.place(place);
        let t = self.place_ty(place);
        let a: Vec<String> = args.iter().map(|x| self.arg(x)).collect();
        let d = dst.map(|d| self.local(d).to_string());
        let elem = t.elem();
        if elem.is_some() && op != RtOp::ArrAppend {
            self.line(&format!("nyrt_arr_unique(&{lv});"));
        }
        let call = match op {
            RtOp::StrAppend => format!("nyrt_str_append(&{lv}, {});", a[0]),
            RtOp::ArrAppend => format!("nyrt_arr_append(&{lv}, {});", a[0]),
            RtOp::ArrPush => format!("nyrt_arr_push(&{lv}, {});", self.addr(&args[0], elem.expect("an array"))),
            RtOp::ArrInsert => {
                format!("nyrt_arr_insert(&{lv}, {}, {}, {at});", a[0], self.addr(&args[1], elem.expect("an array")))
            }
            RtOp::ArrPop | RtOp::ArrRemove => {
                // the element moves out into `dst` (or into a scratch value when it is unused)
                let out = match &d {
                    Some(d) => format!("&{d}"),
                    None => format!("&({}){{0}}", ctype(elem.expect("an array"))),
                };
                if op == RtOp::ArrPop {
                    format!("nyrt_arr_pop(&{lv}, {out}, {at});")
                } else {
                    format!("nyrt_arr_remove(&{lv}, {}, {out}, {at});", a[0])
                }
            }
            RtOp::ArrSort => format!("nyrt_arr_sort(&{lv});"),
            RtOp::ArrReverse => format!("nyrt_arr_reverse(&{lv});"),
            other => unreachable!("{} does not change a place", other.name()),
        };
        self.line(&call);
        self.indent -= 1;
        self.line("}");
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
                self.line("nyrt_buf nyrt_b = nyrt_buf_new();");
                for p in args {
                    let line = self.put("nyrt_buf", p);
                    self.line(&line);
                }
                self.line(&format!("{d} = nyrt_buf_done(&nyrt_b);"));
                self.indent -= 1;
                self.line("}");
                return;
            }
            RtOp::ArrNew => {
                let d = dst.map(|d| self.local(d).to_string()).expect("verified: a destination");
                let t = self.f.local(dst.expect("checked")).ty;
                let elem = t.elem().expect("verified: an array");
                self.line(&format!("{d} = nyrt_arr_new({}, {});", desc(elem), args.len()));
                for x in args {
                    // the array becomes one more owner of each element
                    let line = format!("nyrt_arr_push(&{d}, {});", self.addr(x, elem));
                    self.line(&line);
                }
                return;
            }
            RtOp::ArrGet => {
                let elem = self.ty(&args[0]).elem().expect("verified: an array");
                format!("*({}*)nyrt_arr_at({}, {}, {at})", ctype(elem), a[0], a[1])
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
            RtOp::StrChars => format!("nyrt_str_chars({})", a[0]),
            RtOp::StrCodes => format!("nyrt_str_codes({})", a[0]),
            RtOp::StrSplit => format!("nyrt_str_split({}, {}, {at})", a[0], a[1]),
            RtOp::ArrSlice => format!("nyrt_arr_slice({}, {}, {}, {at})", a[0], a[1], a[2]),
            RtOp::ArrRepeat => format!("nyrt_arr_repeat({}, {}, {at})", a[0], a[1]),
            RtOp::ArrConcat => format!("nyrt_arr_concat({}, {})", a[0], a[1]),
            RtOp::ArrJoin => format!("nyrt_arr_join({}, {})", a[0], a[1]),
            other => unreachable!("{} is a `Mutate`", other.name()),
        };
        let line = self.assign(dst, call);
        self.line(&line);
    }

    /// `nyrt_put_int(x);` / `nyrt_buf_int(&b, x);` for one part of a print or format.
    fn put(&self, prefix: &str, p: &Expr) -> String {
        // the buffer is `nyrt_b`: user names never start with `ny`, so it cannot hide one
        let target = if prefix == "nyrt_buf" { "&nyrt_b, " } else { "" };
        if let Expr::Str(id) = p {
            let s = self.m.str(*id);
            return format!("{prefix}_lit({target}{}, {});", string_lit(s), s.len());
        }
        format!("{prefix}_{}({target}{});", rt_name(self.ty(p)), bare(&self.expr(p)))
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
                    BinOp::DeepEq => format!("nyrt_arr_eq({}, {})", bare(&a), bare(&b)),
                    BinOp::DeepNe => format!("(!nyrt_arr_eq({}, {}))", bare(&a), bare(&b)),
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
                    PureFn::ArrLen => format!("{}->len", self.expr(&args[0])),
                    PureFn::ArrContains | PureFn::ArrIndexOf => {
                        let elem = self.ty(&args[0]).elem().expect("verified: an array");
                        let f = if *p == PureFn::ArrContains { "contains" } else { "index_of" };
                        format!("nyrt_arr_{f}({}, {})", a[0], self.addr(&args[1], elem))
                    }
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
