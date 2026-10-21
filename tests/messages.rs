//! Golden test for the compiler's diagnostics. `tests/messages.txt` holds small bad programs and the
//! exact text `nyra check case.nyra` must print for each, so a change to any message shows up as a
//! diff of that file. After changing a message on purpose, rewrite the expected text with
//!
//!     NYRA_BLESS=1 cargo test --test messages
//!
//! and review the diff. The test also checks the rules every diagnostic must follow: a code, a
//! one-line message, a position and a hint that says how to fix the program.

mod common;

use common::{check_json, nyra, scratch, stderr};

const FILE: &str = "tests/messages.txt";

/// The most characters a message and its hint may have together (about 50 tokens).
const BUDGET: usize = 200;
/// The most characters they may have on average (about 30 tokens).
const MEAN: usize = 120;

struct Case {
    name: String,
    src: String,
    expected: String,
}

/// `\u{XXXX}` in the file stands for the character with that code (for characters you cannot see).
fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find("\\u{") {
        let (head, tail) = rest.split_at(i);
        out.push_str(head);
        match tail[3..].find('}').and_then(|j| u32::from_str_radix(&tail[3..3 + j], 16).ok().map(|c| (j, c))) {
            Some((j, code)) if char::from_u32(code).is_some() => {
                out.push(char::from_u32(code).unwrap());
                rest = &tail[3 + j + 1..];
            }
            _ => {
                out.push_str("\\u{");
                rest = &tail[3..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn escape(s: &str) -> String {
    s.chars().map(|c| if matches!(c, '\u{FEFF}' | '\u{200B}') { format!("\\u{{{:X}}}", c as u32) } else { c.to_string() }).collect()
}

/// The text before the first case, and the cases.
fn parse(text: &str) -> (String, Vec<Case>) {
    let mut header = String::new();
    let mut cases: Vec<Case> = Vec::new();
    let mut in_expected = false;
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("==== ") {
            cases.push(Case { name: name.trim().to_string(), src: String::new(), expected: String::new() });
            in_expected = false;
        } else if line == "----" && !in_expected && !cases.is_empty() {
            in_expected = true;
        } else if let Some(c) = cases.last_mut() {
            let dst = if in_expected { &mut c.expected } else { &mut c.src };
            dst.push_str(line);
            dst.push('\n');
        } else {
            header.push_str(line);
            header.push('\n');
        }
    }
    (header, cases)
}

#[test]
fn diagnostics_match_the_golden_file() {
    let text = std::fs::read_to_string(FILE).unwrap().replace("\r\n", "\n");
    let (header, cases) = parse(&text);
    assert!(cases.len() > 100, "{FILE} should hold the whole corpus, found {} cases", cases.len());
    let bless = std::env::var("NYRA_BLESS").is_ok_and(|v| v != "0");
    let dir = scratch("messages");

    let mut blessed = header.clone();
    let mut failures = Vec::new();
    // the length of each message with its hint, and the ones over the budget
    let mut sizes: Vec<usize> = Vec::new();
    let mut too_long: Vec<String> = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        std::fs::write(dir.join("case.nyra"), unescape(&case.src)).unwrap();
        let out = nyra().current_dir(&dir).args(["check", "--strict", "case.nyra"]).output().unwrap();
        assert_eq!(out.status.code(), Some(1), "{}: should fail to compile", case.name);
        let actual = escape(&stderr(&out));

        // the rules every diagnostic follows, read from the human output
        let blocks: Vec<&str> = actual.split("error[").skip(1).collect();
        assert!(
            !blocks.is_empty(),
            "{}: no error in the output:
{actual}",
            case.name
        );
        for block in &blocks {
            let head = block.lines().next().unwrap_or("");
            let (code, message) = head.split_once("]: ").unwrap_or(("", ""));
            // an agent reads every word: the message and the hint stay short (see `BUDGET`), name
            // no file it cannot see, and call the web functions by the names a program uses
            let hint = block.lines().find_map(|l| l.strip_prefix("  = hint: ")).unwrap_or("");
            let size = message.chars().count() + hint.chars().count();
            sizes.push(size);
            if size > BUDGET {
                too_long.push(format!("{} {code} ({size} chars): {message} | {hint}", case.name));
            }
            for text in [message, hint] {
                assert!(
                    !["src/", "docs/", "tools/", ".rs`", "`web."].iter().any(|p| text.contains(p)),
                    "{}: {code} names a file of the repository or a `web.` function: {text}",
                    case.name
                );
            }
            assert!(
                code.len() == 5 && code.starts_with('E') && code[1..].chars().all(|c| c.is_ascii_digit()),
                "{}: bad code in `error[{head}`",
                case.name
            );
            assert!(!message.trim().is_empty(), "{}: {code} has no message", case.name);
            assert!(block.contains("  --> case.nyra:"), "{}: {code} has no position", case.name);
            assert!(block.contains("  = hint: "), "{}: {code} `{message}` has no hint", case.name);
        }
        // the JSON form carries the same errors (checked on every 12th case: spawning is slow)
        if index % 12 == 0 {
            let (ok, json) = check_json(&dir, "case.nyra");
            assert!(!ok, "{}: --json should report failure", case.name);
            let errors = json.get("errors").and_then(|e| e.as_array()).unwrap_or(&[]);
            assert_eq!(errors.len(), blocks.len(), "{}: the human output and the JSON disagree", case.name);
            for e in errors {
                let hint = e.get("hint").and_then(|h| h.as_str()).unwrap_or("");
                assert!(!hint.trim().is_empty(), "{}: the JSON error has no hint", case.name);
                assert!(e.get("line").and_then(|l| l.as_u64()).is_some_and(|l| l >= 1), "{}: bad line", case.name);
                assert!(e.get("col").and_then(|c| c.as_u64()).is_some_and(|c| c >= 1), "{}: bad col", case.name);
            }
        }

        blessed.push_str(&format!("==== {}\n{}----\n{}", case.name, case.src, actual));
        if actual != case.expected {
            failures.push(format!("--- {}\nexpected:\n{}actual:\n{}", case.name, case.expected, actual));
        }
    }

    // the budget: every message with its hint within `BUDGET` characters (about 50 tokens at four
    // characters a token), and on average within `MEAN` (about 30 tokens)
    assert!(too_long.is_empty(), "{} diagnostics are over {BUDGET} characters:\n{}", too_long.len(), too_long.join("\n"));
    let mean = sizes.iter().sum::<usize>() / sizes.len().max(1);
    assert!(mean <= MEAN, "messages with hints average {mean} characters, over {MEAN}");
    if bless {
        std::fs::write(FILE, blessed).unwrap();
        return;
    }
    assert!(
        failures.is_empty(),
        "{} of {} diagnostics changed (run `NYRA_BLESS=1 cargo test --test messages` to accept the new text):\n\n{}",
        failures.len(),
        cases.len(),
        failures.iter().take(8).cloned().collect::<Vec<_>>().join("\n")
    );
}
