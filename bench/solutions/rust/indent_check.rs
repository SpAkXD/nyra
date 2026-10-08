const SNIPPETS: [&[&str]; 11] = [
    &["0|fn main() {", "4|let xs = [1, 2,", "4|3]", "4|if x {", "8|call(a, [b,", "8|c])", "4|}", "0|}"],
    &["0|f(a, {", "4|b: [1, 2)", "0|})"],
    &["0|g {", "2|h()", "0|}"],
    &["0|a {", "4|b {", "8|c", "4|}", "4|}"],
    &["0|x", "0|)"],
    &["0|k(", "4|m[", "8|n", "4|]"],
    &["2|a"],
    &["0|f() {", "0|}", "0|g [", "4|1, (2", "4|)]"],
    &["0|if a {", "4|b", "0|} else {", "4|c", "0|}"],
    &["0|p {", "3|q)"],
    &["0|r([", "4|s", "0|])", "0|t"],
];

fn closer(open: char) -> char {
    match open {
        '(' => ')',
        '[' => ']',
        _ => '}',
    }
}

fn check(lines: &[&str]) -> Option<String> {
    // stack of (opening bracket, line number, indentation of that line)
    let mut stack: Vec<(char, usize, usize)> = Vec::new();
    let mut prev: Option<(usize, &str)> = None;
    for (idx, line) in lines.iter().enumerate() {
        let l = idx + 1;
        let (n, text) = line.split_once('|').unwrap();
        let indent: usize = n.parse().unwrap();
        let top_before = stack.last().copied();
        for ch in text.chars() {
            match ch {
                '(' | '[' | '{' => stack.push((ch, l, indent)),
                ')' | ']' | '}' => match stack.last() {
                    None => return Some(format!("line {}: unexpected {}", l, ch)),
                    Some(&(o, _, _)) => {
                        if closer(o) != ch {
                            return Some(format!("line {}: expected {} but found {}", l, closer(o), ch));
                        }
                        stack.pop();
                    }
                },
                _ => {}
            }
        }
        let expected = match prev {
            None => 0,
            Some((pi, pt)) => {
                let first = text.chars().next().unwrap();
                if first == ')' || first == ']' || first == '}' {
                    top_before.unwrap().2
                } else if pt.ends_with(|c| c == '(' || c == '[' || c == '{') {
                    pi + 4
                } else {
                    pi
                }
            }
        };
        if indent != expected {
            return Some(format!("line {}: indent {}, expected {}", l, indent, expected));
        }
        prev = Some((indent, text));
    }
    stack.last().map(|&(o, l, _)| format!("line {}: unclosed {}", l, o))
}

fn main() {
    for (k, s) in SNIPPETS.iter().enumerate() {
        match check(s) {
            None => println!("snippet {}: ok", k + 1),
            Some(e) => println!("snippet {}: {}", k + 1, e),
        }
    }
}
