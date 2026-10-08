// string building, split, join, char scans (reference for strings.nyra)
use std::fmt::Write;

fn main() {
    let mut s = String::new();
    for i in 0..2_000_000i64 {
        write!(s, "{i}").unwrap();
        s.push(',');
    }
    println!("{}", s.len());
    let sevens = s.chars().filter(|c| *c == '7').count();
    println!("{sevens}");
    let parts: Vec<String> = s.split(',').map(String::from).collect();
    let mut total: i64 = 0;
    for p in &parts {
        if !p.is_empty() {
            total += p.parse::<i64>().unwrap();
        }
    }
    println!("{total}");
    let mut lines: Vec<String> = Vec::new();
    for i in 0..500_000i64 {
        lines.push(format!("item {i}: {} of {}", i * 3, i % 7));
    }
    let text = lines.join("\n");
    println!("{}", text.len());
}
