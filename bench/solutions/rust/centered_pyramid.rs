fn main() {
    let rows = 5;
    for k in 1..=rows {
        println!("{}{}", " ".repeat(rows - k), "*".repeat(2 * k - 1));
    }
}
