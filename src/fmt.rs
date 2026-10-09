//! `nyra fmt file.nyra`: rewrites a program into canonical form.
//!
//! - every error that has exactly one certain fix is repaired first (as `nyra check --fix`);
//! - `ret` is spelled `return`;
//! - each line is indented by four spaces per level of `{ }`, `( )` and `[ ]` that is open at its
//!   start (a line that continues the line before it, for example one that starts with an
//!   operator or a `.`, gets one more level), trailing spaces are removed, and several empty
//!   lines in a row become one.
//!
//! Only the white space at the start and end of lines and the spelling of `return` change. The
//! result is checked before it is written: it must hold exactly the tokens of the program that
//! was formatted and must compile, so formatting can never change what a program does.

use std::process::ExitCode;

use crate::lexer::{self, StrPart, Tok, Token};
use crate::{diag, fix};

/// The indentation of one level.
const INDENT: &str = "    ";

pub fn run(file: &str, src: &str, out: Option<&str>, json: bool) -> ExitCode {
    // the program must compile, with the fixes that are certain
    let (text, applied) = match crate::compile(src) {
        Ok(_) => (src.to_string(), Vec::new()),
        Err(diags) => match fix::repair(src, diags.clone(), crate::compile) {
            Some(r) => (r.text, r.applied),
            None => {
                if json {
                    println!("{}", diag::render_json(&diags, file));
                } else {
                    eprint!("{}", diag::render_human(&diags, file, src));
                    eprintln!("nyra: {} error(s); `fmt` formats a program that compiles", diags.len());
                }
                return ExitCode::from(1);
            }
        },
    };
    if !applied.is_empty() && !json {
        eprint!("{}", diag::render_warnings_human(&applied, file));
    }
    let formatted = format(&text);
    if let Err(why) = verify(&text, &formatted) {
        eprintln!(
            "nyra: internal error: formatting would change the program ({why}); the file was not touched. Please report this bug"
        );
        return ExitCode::from(2);
    }
    match out {
        Some("-") => print!("{formatted}"),
        Some(path) => {
            if let Err(e) = std::fs::write(path, &formatted) {
                eprintln!("nyra: cannot write `{path}`: {e}");
                return ExitCode::from(2);
            }
        }
        None => {
            if formatted == src {
                eprintln!("nyra: {file} is already formatted");
            } else {
                if let Err(e) = std::fs::write(file, &formatted) {
                    eprintln!("nyra: cannot write `{file}`: {e}");
                    return ExitCode::from(2);
                }
                eprintln!("nyra: formatted {file}");
            }
        }
    }
    if json {
        println!(
            "{{\"ok\":true,\"changed\":{},\"fixed\":{},\"warnings\":{}}}",
            formatted != src,
            applied.len(),
            diag::render_json_warnings(&applied, file)
        );
    }
    ExitCode::SUCCESS
}

/// The canonical form of a program that lexes without errors.
pub fn format(src: &str) -> String {
    let crlf = src.contains("\r\n");
    let (toks, _) = lexer::lex(src);
    let lines: Vec<&str> = src.lines().collect();

    // the indentation level of every line that starts with a token
    let mut level: Vec<Option<usize>> = vec![None; lines.len() + 2];
    // that line starts with a closing bracket
    let mut closing: Vec<bool> = vec![false; lines.len() + 2];
    let mut stack: Vec<Tok> = Vec::new();
    let mut prev: Option<&Tok> = None;
    // a line break (a statement end) came after the previous token
    let mut broke = true;
    let mut last_line = 0;
    // the `ret` tokens to spell out: (line, column)
    let mut rets: Vec<(usize, usize)> = Vec::new();
    for t in &toks {
        match t.tok {
            Tok::Newline => {
                broke = true;
                continue;
            }
            Tok::Eof => break,
            _ => {}
        }
        let closes = matches!(t.tok, Tok::RBrace | Tok::RParen | Tok::RBracket);
        if t.span.line != last_line {
            let open = stack.len() - usize::from(closes && !stack.is_empty());
            // a line that goes on with the line before it: no line break between them (an
            // operator or a `.` at the end or start), directly in a block or at the top level
            let goes_on =
                (!broke || matches!(prev, Some(Tok::Comma))) && prev.is_some() && matches!(stack.last(), None | Some(Tok::LBrace));
            if let Some(slot) = level.get_mut(t.span.line) {
                *slot = Some(open + usize::from(goes_on));
                closing[t.span.line] = closes;
            }
            last_line = t.span.line;
        }
        match t.tok {
            Tok::LBrace | Tok::LParen | Tok::LBracket => stack.push(t.tok.clone()),
            Tok::RBrace | Tok::RParen | Tok::RBracket => {
                stack.pop();
            }
            Tok::Ret => rets.push((t.span.line, t.span.col)),
            _ => {}
        }
        broke = false;
        prev = Some(&t.tok);
    }

    // a line with only a comment is indented like the next line of code
    // (before a closing bracket it belongs to the block that ends there)
    let mut next_level = 0;
    for i in (1..=lines.len()).rev() {
        match level[i] {
            Some(l) => next_level = l + usize::from(closing[i]),
            None if !lines[i - 1].trim().is_empty() => level[i] = Some(next_level),
            None => {}
        }
    }

    let mut out = String::new();
    for (i, raw) in lines.iter().enumerate() {
        let no = i + 1;
        let mut line: Vec<char> = raw.chars().collect();
        // `ret` becomes `return`, from the right so the columns before stay valid
        for &(l, c) in rets.iter().rev().filter(|(l, _)| *l == no) {
            let _ = l;
            if line.get(c - 1..c + 2).is_some_and(|w| w == ['r', 'e', 't'])
                && !line.get(c + 2).is_some_and(|x| x.is_alphanumeric() || *x == '_')
            {
                line.splice(c + 2..c + 2, ['u', 'r', 'n']);
            }
        }
        let text: String = line.into_iter().collect();
        let body = text.trim();
        if body.is_empty() {
            // runs of empty lines become one, and the file does not start with one
            if out.is_empty() || out.ends_with("\n\n") {
                continue;
            }
        } else {
            for _ in 0..level[no].unwrap_or(0) {
                out.push_str(INDENT);
            }
            out.push_str(body);
        }
        out.push('\n');
    }
    // one line break at the end, and no empty lines after the last line of code
    while out.ends_with("\n\n") {
        out.pop();
    }
    if crlf {
        out = out.replace('\n', "\r\n");
    }
    out
}

/// The formatted text must lex to the same tokens as the original (`ret` and `return` are one
/// token) and compile.
fn verify(before: &str, after: &str) -> Result<(), String> {
    let (a, ea) = lexer::lex(before);
    let (b, eb) = lexer::lex(after);
    if !ea.is_empty() || !eb.is_empty() {
        return Err("the text does not lex".into());
    }
    if !same_tokens(&a, &b) {
        return Err("the tokens differ".into());
    }
    if crate::compile(after).is_err() {
        return Err("the result does not compile".into());
    }
    Ok(())
}

/// True if the tokens are the same, ignoring where they are.
fn same_tokens(a: &[Token], b: &[Token]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| match (&x.tok, &y.tok) {
            (Tok::Interp(p), Tok::Interp(q)) => {
                p.len() == q.len()
                    && p.iter().zip(q).all(|(m, n)| match (m, n) {
                        (StrPart::Lit(s), StrPart::Lit(t)) => s == t,
                        (StrPart::Code(s, _), StrPart::Code(t, _)) => s == t,
                        _ => false,
                    })
            }
            (p, q) => p == q,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indents_and_spells_return() {
        let src = "fn f(x: int) -> int {\n  if x > 0 {\n\t\tret 1\n    }\n  ret 0   \n}\n\n\n";
        assert_eq!(format(src), "fn f(x: int) -> int {\n    if x > 0 {\n        return 1\n    }\n    return 0\n}\n");
    }

    #[test]
    fn continuation_lines_get_one_more_level() {
        let src = "fn main() {\nlet s = [1, 2]\n.len()\nprint(\ns,\n  1\n)\n}\n";
        assert_eq!(format(src), "fn main() {\n    let s = [1, 2]\n        .len()\n    print(\n        s,\n        1\n    )\n}\n");
    }

    #[test]
    fn comments_follow_the_next_line() {
        let src = "fn main() {\n// note\n        print(1) // x\n// end\n}\n";
        assert_eq!(format(src), "fn main() {\n    // note\n    print(1) // x\n    // end\n}\n");
    }

    #[test]
    fn a_word_that_starts_with_ret_stays() {
        let src = "fn main() {\nlet retry = 1\nprint(retry)\n}\n";
        assert_eq!(format(src), "fn main() {\n    let retry = 1\n    print(retry)\n}\n");
    }
}
