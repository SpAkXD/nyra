fn encode(text: &str) -> String {
    let cs: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        let mut j = i;
        while j < cs.len() && cs[j] == c {
            j += 1;
        }
        let n = j - i;
        if c == '~' {
            out.push_str(&format!("~{}~", n));
        } else if n >= 4 {
            out.push_str(&format!("~{}{}", n, c));
        } else {
            for _ in 0..n {
                out.push(c);
            }
        }
        i = j;
    }
    out
}

fn plain(c: char) -> bool {
    c.is_ascii_lowercase() || c == '.'
}

fn decode(code: &str) -> Option<String> {
    let cs: Vec<char> = code.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c == '~' {
            i += 1;
            if i >= cs.len() || !cs[i].is_ascii_digit() || cs[i] == '0' {
                return None;
            }
            let mut n: usize = 0;
            while i < cs.len() && cs[i].is_ascii_digit() {
                n = n * 10 + cs[i].to_digit(10).unwrap() as usize;
                i += 1;
            }
            if i >= cs.len() {
                return None;
            }
            let r = cs[i];
            if !(plain(r) || r == '~') {
                return None;
            }
            for _ in 0..n {
                out.push(r);
            }
            i += 1;
        } else if plain(c) {
            out.push(c);
            i += 1;
        } else {
            return None;
        }
    }
    Some(out)
}

fn main() {
    let texts = ["aaaabbbcccccd", "~~x", ".......", "abc", "zzzzzzzzzzzzzz~", "a~~~~b"];
    for t in texts.iter() {
        println!("encode {} -> {}", t, encode(t));
    }
    let codes = [
        "~4a~3~b", "~04a", "~2ab", "a~b", "~12z", "~5", "ab~3~~", "x~1~y", "q7", "~10.~4~", "~1~~1~", "a~3B",
    ];
    for c in codes.iter() {
        match decode(c) {
            None => println!("decode {} -> invalid", c),
            Some(t) => {
                if encode(&t) == *c {
                    println!("decode {} -> {}", c, t);
                } else {
                    println!("decode {} -> {} (non-canonical)", c, t);
                }
            }
        }
    }
}
