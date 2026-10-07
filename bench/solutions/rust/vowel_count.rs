fn main() {
    let text = "the quick brown fox jumps over the lazy dog";
    let count = text.chars().filter(|c| "aeiou".contains(*c)).count();
    println!("{}", count);
}
