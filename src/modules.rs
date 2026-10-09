//! Modules of your own: `use ./name` imports `name.nyra`, a file next to the one being compiled
//! (`./folder/name` and `../name` work too). Its `pub` functions are called `name.f(x)`; its structs
//! and enums (they keep their plain names) are used by the importer without any prefix.
//!
//! The loader runs right after parsing. It parses every imported file once (an import cycle is
//! E0303), renames the file's functions to `name.f` (the way the standard modules' functions are
//! named, so no backend needs to know), turns each `name.f(x)` of the importer into a call of that
//! function, and adds the file's functions, structs, enums and examples to the program. A private
//! function (`fn` without `pub`) cannot be called from another file (E0301).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::ast::{each_stmt_mut, EnumDef, Example, Expr, ExprKind, Func, Program, StructDef, Use};
use crate::diag::{suggest, Diag};
use crate::{lexer, parser, stdlib};

thread_local! {
    /// The file being compiled: its imports are relative to its folder.
    static MAIN: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// Tells the loader which file is being compiled (`None`: a program given as text, which has no
/// folder, so it cannot import files).
pub fn set_main(path: Option<&Path>) {
    MAIN.with(|m| *m.borrow_mut() = path.map(Path::to_path_buf));
}

/// What other files may know about a loaded module.
struct Loaded {
    file: PathBuf,
    /// every function of the module, by its own name
    funcs: HashSet<String>,
    /// the ones marked `pub`
    public: HashSet<String>,
}

#[derive(Default)]
struct Loader {
    loaded: HashMap<String, Loaded>,
    /// the files being loaded right now (outermost first), to find import cycles
    stack: Vec<PathBuf>,
    errs: Vec<Diag>,
    /// what the modules add to the program
    uses: Vec<Use>,
    funcs: Vec<Func>,
    structs: Vec<StructDef>,
    enums: Vec<EnumDef>,
    examples: Vec<Example>,
    files: HashMap<String, String>,
}

/// Which names a file's code means: its own functions (renamed), and the modules it imports.
struct Scope<'a> {
    /// the module's name, `None` for the program's own file
    module: Option<&'a str>,
    own: &'a HashSet<String>,
    imports: &'a [String],
    /// the file as shown in messages, `None` for the program's own file
    shown: Option<&'a str>,
}

/// Loads the files that `prog` imports (`use ./name`), and the ones they import.
pub fn load(prog: &mut Program) -> Vec<Diag> {
    let wanted: Vec<Use> = prog.uses.iter().filter(|u| u.path.is_some()).cloned().collect();
    if wanted.is_empty() {
        return Vec::new();
    }
    prog.uses.retain(|u| u.path.is_none());
    let Some(main) = MAIN.with(|m| m.borrow().clone()) else {
        return wanted
            .iter()
            .map(|u| {
                Diag::new("E0300", format!("cannot import `{}` here: this program has no folder", u.module), u.span)
                    .hint("imports of your own files work for a program that is run from a file: `nyra run main.nyra`")
            })
            .collect();
    };
    let mut l = Loader::default();
    l.stack.push(main.canonicalize().unwrap_or_else(|_| main.clone()));
    let mut imports = Vec::new();
    for u in &wanted {
        if let Some(name) = l.import(u, &main, None) {
            if imports.contains(&name) {
                l.errs.push(Diag::new("E0304", format!("`{name}` is imported twice"), u.span).hint("one `use` line per module is enough"));
            } else {
                imports.push(name);
            }
        }
    }
    // the program's own code calls the modules as `name.f(x)`
    let own = HashSet::new();
    let scope = Scope { module: None, own: &own, imports: &imports, shown: None };
    for f in &mut prog.funcs {
        l.rewrite_func(f, &scope);
    }
    for ex in &mut prog.examples {
        l.rewrite_expr(&mut ex.expr, &scope);
    }
    for u in std::mem::take(&mut l.uses) {
        if !prog.uses.iter().any(|x| x.module == u.module) {
            prog.uses.push(u);
        }
    }
    prog.funcs.append(&mut l.funcs);
    prog.structs.append(&mut l.structs);
    prog.enums.append(&mut l.enums);
    prog.examples.append(&mut l.examples);
    prog.files.extend(l.files);
    l.errs
}

impl Loader {
    fn err(&mut self, code: &'static str, msg: String, hint: String, u: &Use, shown: Option<&str>) {
        let mut d = Diag::new(code, msg, u.span).hint(hint);
        d.file = shown.map(str::to_string);
        self.errs.push(d);
    }

    /// Loads the module of the `use` line `u`, which stands in the file `from`. Gives its name.
    fn import(&mut self, u: &Use, from: &Path, shown: Option<&str>) -> Option<String> {
        let path = u.path.as_deref()?;
        let name = u.module.clone();
        let ok = (path.starts_with("./") || path.starts_with("../")) && !path.contains('\\') && !path.ends_with(".nyra") && !path.contains("//");
        if !ok {
            self.err(
                "E0305",
                format!("`{path}` is not a path Nyra imports"),
                "write the path from the importing file's folder, with `/` and without `.nyra`: `use ./shapes`, `use ../util/text`".to_string(),
                u,
                shown,
            );
            return None;
        }
        let file = tidy(&from.parent().unwrap_or(Path::new(".")).join(format!("{path}.nyra")));
        let Ok(canon) = file.canonicalize() else {
            self.err(
                "E0300",
                format!("module `{name}` not found: there is no file `{}`", slashes(&file)),
                format!("a `use {path}` line imports the file `{name}.nyra` from the folder of the importing file"),
                u,
                shown,
            );
            return None;
        };
        if let Some(at) = self.stack.iter().position(|s| *s == canon) {
            let mut chain: Vec<String> = self.stack[at..].iter().map(|p| stem(p)).collect();
            chain.push(stem(&canon));
            self.err(
                "E0303",
                format!("import cycle: {}", chain.join(" -> ")),
                "move what both files need into a third file that imports neither".to_string(),
                u,
                shown,
            );
            return None;
        }
        if stdlib::is_module(&name) {
            self.err(
                "E0304",
                format!("`{name}` is also the name of a standard module"),
                format!("rename the file (and the `use` line): a module is called by its name, `{name}.f()`"),
                u,
                shown,
            );
            return None;
        }
        if let Some(l) = self.loaded.get(&name) {
            if l.file == canon {
                return Some(name);
            }
            self.err(
                "E0304",
                format!("two different files are both imported as `{name}`"),
                "give one of the files another name: a module is called by its file's name".to_string(),
                u,
                shown,
            );
            return None;
        }
        let shown_file = slashes(&file);
        let text = match std::fs::read_to_string(&canon) {
            Ok(t) => t,
            Err(e) => {
                self.err("E0300", format!("cannot read `{shown_file}`: {e}"), "check that the file is readable text".to_string(), u, shown);
                return None;
            }
        };
        let (toks, errs) = lexer::lex(&text);
        let mut errs = errs;
        let mut sub = None;
        if errs.is_empty() {
            let (p, perrs) = parser::parse(toks);
            errs = perrs;
            sub = Some(p);
        }
        if !errs.is_empty() {
            for mut d in errs {
                d.file = Some(shown_file.clone());
                self.errs.push(d);
            }
            return None;
        }
        let mut sub = sub?;
        if sub.script {
            // the statements of a script became its `main`
            let at = sub.funcs.iter().find(|f| f.name == "main").map(|f| f.span).unwrap_or(u.span);
            let mut d = Diag::new("E0285", format!("module `{name}` has statements at the top level"), at)
                .hint("a module holds only definitions (`fn`, `struct`, `enum`, `ex`); the program that imports it has the statements");
            d.file = Some(shown_file);
            self.errs.push(d);
            return None;
        }
        self.stack.push(canon.clone());
        let mut imports = Vec::new();
        for su in sub.uses.iter().filter(|u| u.path.is_some()).cloned().collect::<Vec<_>>() {
            if let Some(n) = self.import(&su, &file, Some(&shown_file)) {
                if !imports.contains(&n) {
                    imports.push(n);
                }
            }
        }
        self.stack.pop();
        self.uses.extend(sub.uses.iter().filter(|u| u.path.is_none()).cloned());
        let own: HashSet<String> = sub.funcs.iter().map(|f| f.name.clone()).collect();
        let public: HashSet<String> = own.iter().filter(|f| sub.public.contains(*f)).cloned().collect();
        self.loaded.insert(name.clone(), Loaded { file: canon, funcs: own.clone(), public });
        let scope = Scope { module: Some(&name), own: &own, imports: &imports, shown: Some(&shown_file) };
        for f in &mut sub.funcs {
            self.rewrite_func(f, &scope);
        }
        for ex in &mut sub.examples {
            self.rewrite_expr(&mut ex.expr, &scope);
            ex.file = Some(shown_file.clone());
        }
        for f in &mut sub.funcs {
            f.name = format!("{name}.{}", f.name);
            self.files.insert(f.name.clone(), shown_file.clone());
        }
        self.funcs.append(&mut sub.funcs);
        self.structs.append(&mut sub.structs);
        self.enums.append(&mut sub.enums);
        self.examples.append(&mut sub.examples);
        Some(name)
    }

    fn rewrite_func(&mut self, f: &mut Func, scope: &Scope) {
        let mut errs = Vec::new();
        each_stmt_mut(&mut f.body, &mut |_| {}, &mut |e| fix_expr(e, scope, &self.loaded, &mut errs));
        self.errs.append(&mut errs);
    }

    fn rewrite_expr(&mut self, e: &mut Expr, scope: &Scope) {
        let mut errs = Vec::new();
        e.each_mut(&mut |x| fix_expr(x, scope, &self.loaded, &mut errs));
        self.errs.append(&mut errs);
    }
}

/// The path without its `.` parts: `dir/./a.nyra` is `dir/a.nyra`.
fn tidy(p: &Path) -> PathBuf {
    p.components().filter(|c| !matches!(c, std::path::Component::CurDir)).collect()
}

/// A path as shown in messages: always with `/`.
fn slashes(p: &Path) -> String {
    p.display().to_string().replace('\\', "/")
}

/// The file name without the folder and the extension: `shapes`.
fn stem(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// `name.f(x)` becomes a call of the function `name.f`; a call of the module's own function gets
/// the module's name too.
fn fix_expr(e: &mut Expr, scope: &Scope, loaded: &HashMap<String, Loaded>, errs: &mut Vec<Diag>) {
    let span = e.span;
    let mut err = |code: &'static str, msg: String, hint: String| {
        let mut d = Diag::new(code, msg, span).hint(hint);
        d.file = scope.shown.map(str::to_string);
        errs.push(d);
    };
    match &mut e.kind {
        ExprKind::Call(name, _) if scope.own.contains(name.as_str()) => {
            if let Some(m) = scope.module {
                *name = format!("{m}.{name}");
            }
        }
        ExprKind::Method(recv, f, args) => {
            let ExprKind::Var(m) = &recv.kind else { return };
            if !scope.imports.contains(m) {
                return;
            }
            let Some(l) = loaded.get(m.as_str()) else { return };
            if !l.funcs.contains(f.as_str()) {
                let names: Vec<&str> = l.public.iter().map(String::as_str).collect();
                let hint = match suggest(f, names.iter().copied()) {
                    Some(s) => s,
                    None => {
                        let mut sorted = names.clone();
                        sorted.sort_unstable();
                        format!("the public functions of `{m}` are {}", sorted.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", "))
                    }
                };
                err("E0306", format!("module `{m}` has no `{f}`"), hint);
            } else if !l.public.contains(f.as_str()) {
                err(
                    "E0301",
                    format!("`{f}` is private to module `{m}`"),
                    format!("mark it `pub` in `{m}.nyra` (`pub fn {f}(...)`) if other files may call it"),
                );
            } else {
                let args = std::mem::take(args);
                e.kind = ExprKind::Call(format!("{m}.{f}"), args);
            }
        }
        ExprKind::Field(recv, f) => {
            let ExprKind::Var(m) = &recv.kind else { return };
            if scope.imports.contains(m) && loaded.contains_key(m.as_str()) {
                err("E0307", format!("`{m}.{f}` is not a value"), format!("call a function of the module: `{m}.{f}(...)`; types are used by their plain names"));
            }
        }
        _ => {}
    }
}
