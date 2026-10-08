use std::collections::BTreeMap;

const LINES: [&str; 28] = [
    "; settings",
    "title = demo",
    "[server]",
    "host = example.org",
    "port = 8080",
    "name = \"  main  \"",
    "port = 9090",
    "[paths]",
    "root = /srv",
    "logs = ${paths.root}/logs",
    "data = ${root}/data",
    "bad line here",
    "[ server ]",
    "url = http://${host}:${port}/",
    "alias = ${missing}",
    "= value",
    "   # indented comment",
    "",
    "[paths]",
    "tags += web",
    "tags += api",
    "root = /var",
    "quoted = \"${root}\"",
    "[extra]",
    "formula = x=y+1",
    "mirror = ${server.url}${paths.root}",
    "broken = ${paths.data}${server.nope}",
    "half = \"open",
];

// Replaces every reference in `value`; Err(reference text) for the first unknown one.
fn expand(value: &str, section: &str, settings: &BTreeMap<(String, String), String>) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = value;
    while let Some(start) = rest.find("${") {
        let after = &rest[start + 2..];
        let close = match after.find('}') {
            Some(c) => c,
            None => break,
        };
        out.push_str(&rest[..start]);
        let reference = &after[..close];
        let (sec, key) = match reference.find('.') {
            Some(d) => (&reference[..d], &reference[d + 1..]),
            None => (section, reference),
        };
        match settings.get(&(sec.to_string(), key.to_string())) {
            Some(v) => out.push_str(v),
            None => return Err(reference.to_string()),
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

fn main() {
    let mut settings: BTreeMap<(String, String), String> = BTreeMap::new();
    let mut section: Option<String> = None;
    for (i, raw) in LINES.iter().enumerate() {
        let n = i + 1;
        let line = raw.trim_matches(' ');
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') && line.len() >= 2 {
            section = Some(line[1..line.len() - 1].trim_matches(' ').to_string());
            continue;
        }
        let eq = match raw.find('=') {
            Some(e) => e,
            None => {
                println!("line {}: syntax error", n);
                continue;
            }
        };
        let mut before = &raw[..eq];
        let append = before.ends_with('+');
        if append {
            before = &before[..before.len() - 1];
        }
        let key = before.trim_matches(' ');
        let value = raw[eq + 1..].trim_matches(' ');
        if key.is_empty() {
            println!("line {}: empty key", n);
            continue;
        }
        let sec = match &section {
            Some(s) => s.clone(),
            None => {
                println!("line {}: no section", n);
                continue;
            }
        };
        let value = if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
            value[1..value.len() - 1].to_string()
        } else {
            match expand(value, &sec, &settings) {
                Ok(v) => v,
                Err(r) => {
                    println!("line {}: unknown reference {}", n, r);
                    continue;
                }
            }
        };
        let k = (sec, key.to_string());
        let new = match (append, settings.get(&k)) {
            (true, Some(old)) => format!("{},{}", old, value),
            _ => value,
        };
        settings.insert(k, new);
    }
    println!("---");
    for ((s, k), v) in &settings {
        println!("{}.{} = [{}]", s, k, v);
    }
}
