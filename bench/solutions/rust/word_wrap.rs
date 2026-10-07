fn main() {
    let text = "the quick brown fox jumps over the lazy dog and keeps running far away from here";
    let width = 18;
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if current.is_empty() {
            current.push_str(word);
        } else if current.chars().count() + 1 + word.chars().count() <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(current);
            current = word.to_string();
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    for line in &lines {
        println!("{}", line);
    }
    println!("lines: {}", lines.len());
}
