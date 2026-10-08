const ROWS: [(&str, i64, i64, &str); 6] = [
    ("bolts", 12000, 5, "bulk"),
    ("hex nuts", 350, 12, "galvanized"),
    ("washers", 7, 123456, "special order"),
    ("anchor", 1, 99, ""),
    ("rivets", 1500, 3, "aluminium"),
    ("brackets", 24, 1050, "left-hand only"),
];

#[derive(Clone, Copy)]
enum Align {
    Left,
    Right,
    Center,
}

const ALIGNS: [Align; 5] = [Align::Left, Align::Right, Align::Right, Align::Right, Align::Center];

fn group(n: i64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

fn dollars(c: i64) -> String {
    format!("{}.{:02}", group(c / 100), c % 100)
}

fn note(s: &str) -> String {
    if s.chars().count() > 10 {
        let mut t: String = s.chars().take(9).collect();
        t.push('~');
        t
    } else {
        s.to_string()
    }
}

fn pad(s: &str, w: usize, a: Align) -> String {
    let p = w - s.chars().count();
    match a {
        Align::Left => format!("{}{}", s, " ".repeat(p)),
        Align::Right => format!("{}{}", " ".repeat(p), s),
        Align::Center => format!("{}{}{}", " ".repeat(p / 2), s, " ".repeat(p - p / 2)),
    }
}

fn main() {
    let header: Vec<String> = ["item", "qty", "price", "amount", "note"].iter().map(|s| s.to_string()).collect();
    let mut data: Vec<Vec<String>> = Vec::new();
    let (mut tq, mut ta) = (0, 0);
    for &(item, q, p, n) in ROWS.iter() {
        tq += q;
        ta += q * p;
        data.push(vec![item.to_string(), group(q), dollars(p), dollars(q * p), note(n)]);
    }
    let total = vec!["TOTAL".to_string(), group(tq), String::new(), dollars(ta), String::new()];
    let mut widths = [0usize; 5];
    for row in std::iter::once(&header).chain(data.iter()).chain(std::iter::once(&total)) {
        for (i, c) in row.iter().enumerate() {
            widths[i] = widths[i].max(c.chars().count());
        }
    }
    let border = |ch: &str| {
        let mut s = String::from("+");
        for w in widths.iter() {
            s.push_str(&ch.repeat(w + 2));
            s.push('+');
        }
        s
    };
    let line = |row: &Vec<String>| {
        let cells: Vec<String> = row.iter().enumerate().map(|(i, c)| pad(c, widths[i], ALIGNS[i])).collect();
        format!("| {} |", cells.join(" | "))
    };
    println!("{}", border("-"));
    println!("{}", line(&header));
    println!("{}", border("="));
    for row in &data {
        println!("{}", line(row));
    }
    println!("{}", border("-"));
    println!("{}", line(&total));
    println!("{}", border("-"));
}
