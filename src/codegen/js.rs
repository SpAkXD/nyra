//! JavaScript backend. Output runs on Node.js or in the browser.

use std::fmt::Write;

use super::{bare, names};
use crate::ir::{BinOp, Expr, Func, LocalId, Module, Place, PureFn, RtOp, Step, Stmt, StmtKind, Structs, Ty, UnOp};

const RESERVED: &[&str] = &[
    "arguments", "await", "break", "case", "catch", "class", "const", "continue", "debugger", "default",
    "delete", "do", "else", "enum", "eval", "export", "extends", "false", "finally", "for", "function",
    "if", "implements", "import", "in", "instanceof", "interface", "let", "new", "null", "package",
    "private", "protected", "public", "return", "static", "super", "switch", "this", "throw", "true",
    "try", "typeof", "var", "void", "while", "with", "yield", "undefined", "NaN", "Infinity", "console",
    "Math", "String", "Number", "Object", "Array", "JSON", "Symbol", "BigInt", "Error", "RangeError", "globalThis",
    "process", "require", "module", "exports", "NyPanic", "NY_SURR", "NY_ESC",
];

/// The JavaScript runtime, emitted before every program (`@FILE@` becomes the source path).
const PRELUDE: &str = include_str!("../rt/js/core.js");
const STRINGS: &str = include_str!("../rt/js/str.js");
const ARRAYS: &str = include_str!("../rt/js/arr.js");

fn name(n: &str) -> String {
    if RESERVED.contains(&n) || n.starts_with("ny_") {
        format!("{n}_")
    } else {
        n.to_string()
    }
}

/// The type descriptor `ny_fmt` prints a value with (a char is a number in JavaScript).
fn tdesc(t: Ty) -> String {
    match t {
        Ty::Int => "i".into(),
        Ty::Float => "f".into(),
        Ty::Bool => "b".into(),
        Ty::Char => "c".into(),
        Ty::Str => "s".into(),
        Ty::Array(_) => format!("[{}", tdesc(t.elem().expect("an array"))),
        _ => "S".into(),
    }
}

/// `file` is the source path as given to nyra; runtime errors report it.
pub fn gen(m: &Module, file: &str) -> String {
    let mut out = PRELUDE.replace("@FILE@", &crate::diag::json_str(file));
    out.push('\n');
    out.push_str(STRINGS);
    out.push_str(ARRAYS);
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
    /// Counter for helper names (loop variables, element references).
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

    fn fresh(&mut self, prefix: &str) -> String {
        self.tmp += 1;
        format!("ny_{prefix}{}", self.tmp)
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

    /// A value that is about to get one more owner: an aggregate is marked shared.
    fn owned(&self, e: &Expr) -> String {
        if Structs::aggregate(self.ty(e)) {
            format!("ny_sh({})", self.arg(e))
        } else {
            self.arg(e)
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
                let args: Vec<String> = args.iter().map(|a| self.arg(a)).collect();
                let line = self.assign(*dst, format!("{}({})", name(&self.m.func(*func).name), args.join(", ")));
                self.line(&line);
            }
            StmtKind::Op { dst, op, args } => self.op(*dst, *op, args, &at),
            StmtKind::Store { place, value } => {
                self.line("{");
                self.indent += 1;
                let lv = self.place(place, false);
                let line = format!("{lv} = {};", self.owned(value));
                self.line(&line);
                self.indent -= 1;
                self.line("}");
            }
            StmtKind::Mutate { dst, op, place, args } => {
                self.line("{");
                self.indent += 1;
                // a string is replaced, an array is changed where it is (after copying it if shared)
                let target = self.place(place, *op != RtOp::StrAppend);
                let a: Vec<String> = args.iter().map(|x| self.arg(x)).collect();
                let code = match op {
                    RtOp::StrAppend => format!("{target} += {}", a[0]),
                    RtOp::ArrPush => format!("{target}.push({})", self.owned(&args[0])),
                    RtOp::ArrPop => format!("ny_pop({target}, {at})"),
                    RtOp::ArrInsert => format!("ny_insert({target}, {}, {}, {at})", a[0], self.owned(&args[1])),
                    RtOp::ArrRemove => format!("ny_remove({target}, {}, {at})", a[0]),
                    RtOp::ArrSort => {
                        let elem = self.place_ty(place).elem().expect("verified: an array");
                        let lt = if elem == Ty::Str { "ny_lt_str" } else { "ny_lt_num" };
                        format!("ny_sort({target}, {lt})")
                    }
                    RtOp::ArrReverse => format!("{target}.reverse()"),
                    RtOp::ArrAppend => format!("ny_append({target}, {})", a[0]),
                    other => unreachable!("{} does not change a place", other.name()),
                };
                let line = self.assign(*dst, code);
                self.line(&line);
                self.indent -= 1;
                self.line("}");
            }
            StmtKind::If { .. } => self.if_chain(s),
            StmtKind::Loop { head, cond, body, step } => self.lp(head, cond, body, step),
            StmtKind::ForEach { var, iter, body } => {
                // `for...of` walks a string by code points (like Nyra) and an array by elements
                let c = self.fresh("c");
                let line = format!("for (const {c} of {}) {{", self.arg(iter));
                self.line(&line);
                self.indent += 1;
                let value = if self.ty(iter) == Ty::Str { format!("{c}.codePointAt(0)") } else { c };
                let line = format!("{} = {value};", self.local(*var));
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
            // the garbage collector frees memory; a second owner of an aggregate marks it shared
            StmtKind::Dup(l) => {
                if Structs::aggregate(self.f.local(*l).ty) {
                    let line = format!("ny_sh({});", self.local(*l));
                    self.line(&line);
                }
            }
            StmtKind::Drop(_) => {}
            StmtKind::Free(l) => {
                let line = format!("{} = undefined;", self.local(*l));
                self.line(&line);
            }
        }
    }

    /// Emits the copy-on-write steps for a place. Returns an assignable expression for it, or,
    /// with `unique`, the place's own array, unique so it can be changed.
    fn place(&mut self, p: &Place, unique: bool) -> String {
        let mut lv = self.local(p.root).to_string();
        let mut t = self.f.local(p.root).ty;
        if p.path.is_empty() {
            if unique && Structs::aggregate(t) {
                self.line(&format!("if ({lv}.ny_s) {lv} = ny_cp({lv});"));
            }
            return lv;
        }
        // the root changes below this point
        self.line(&format!("if ({lv}.ny_s) {lv} = ny_cp({lv});"));
        let n = p.path.len();
        for (k, step) in p.path.iter().enumerate() {
            let key = match step {
                Step::Index(i, span) => {
                    t = t.elem().expect("verified: an array");
                    format!("ny_ck({lv}, {}, {}, {})", self.arg(i), span.line, span.col)
                }
                Step::Field(_) => unreachable!("structs are not lowered yet"),
            };
            if k + 1 == n && !unique {
                return format!("{lv}[{key}]");
            }
            let r = self.fresh("p");
            self.line(&format!("const {r} = ny_u({lv}, {key});"));
            lv = r;
        }
        let _ = t;
        lv
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

    fn op(&mut self, dst: Option<LocalId>, op: RtOp, args: &[Expr], at: &str) {
        let a: Vec<String> = args.iter().map(|x| self.arg(x)).collect();
        let code = match op {
            RtOp::Print => self.print(args),
            RtOp::Format => self.template(args),
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
            RtOp::StrChars | RtOp::StrCodes => format!("ny_chars({})", a[0]),
            RtOp::StrSplit => format!("ny_split({}, {}, {at})", a[0], a[1]),
            RtOp::ArrNew => {
                let items: Vec<String> = args.iter().map(|x| self.owned(x)).collect();
                format!("[{}]", items.join(", "))
            }
            RtOp::ArrGet => format!("ny_get({}, {}, {at})", a[0], a[1]),
            RtOp::ArrSlice => format!("ny_aslice({}, {}, {}, {at})", a[0], a[1], a[2]),
            RtOp::ArrRepeat => format!("ny_arep({}, {}, {at})", a[0], a[1]),
            RtOp::ArrConcat => format!("ny_aconcat({}, {})", a[0], a[1]),
            RtOp::ArrJoin => {
                if self.ty(&args[0]).elem() == Some(Ty::Char) {
                    format!("ny_join_char({}, {})", a[0], a[1])
                } else {
                    format!("{}.join({})", self.expr(&args[0]), a[1])
                }
            }
            other => unreachable!("{} is a `Mutate`", other.name()),
        };
        let line = self.assign(dst, code);
        self.line(&line);
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

    /// The text of one value as `print` shows it (numbers through `String()`, so `-0` is `0`).
    fn shown(&self, p: &Expr) -> String {
        let x = self.arg(p);
        match self.ty(p) {
            Ty::Int | Ty::Float => format!("String({x})"),
            Ty::Char => format!("ny_char_str({x})"),
            Ty::Bool | Ty::Str => x,
            t => format!("ny_fmt({x}, \"{}\")", tdesc(t)),
        }
    }

    fn print(&self, parts: &[Expr]) -> String {
        if let [p] = parts {
            return format!("console.log({})", self.shown(p));
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
            match self.ty(p) {
                Ty::Int | Ty::Float | Ty::Bool | Ty::Str => s.push_str(&self.arg(p)),
                _ => s.push_str(&self.shown(p)),
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
                    BinOp::DeepEq => format!("ny_eq({}, {})", bare(&a), bare(&b)),
                    BinOp::DeepNe => format!("(!ny_eq({}, {}))", bare(&a), bare(&b)),
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
                    PureFn::ArrLen => format!("{}.length", self.expr(&args[0])),
                    PureFn::ArrContains => format!("(ny_aindex({}, {}) >= 0)", a[0], a[1]),
                    PureFn::ArrIndexOf => format!("ny_aindex({}, {})", a[0], a[1]),
                }
            }
        }
    }
}
