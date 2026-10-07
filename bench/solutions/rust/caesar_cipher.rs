fn main() {
    let text = "hello world";
    let mut result = String::new();
    for ch in text.chars() {
        if ch.is_ascii_lowercase() {
            let shifted = (ch as u8 - b'a' + 3) % 26;
            result.push((b'a' + shifted) as char);
        } else {
            result.push(ch);
        }
    }
    println!("{}", result);
}
