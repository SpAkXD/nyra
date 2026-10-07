fn main() {
    let text = "hello world";
    let mut result = String::new();
    for ch in text.chars() {
        result.insert(0, ch);
    }
    println!("{}", result);
}
