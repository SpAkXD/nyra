fn main() {
    let mut x: u64 = 1;
    for _ in 0..10 {
        x = (x * 75 + 74) % 65537;
        println!("{}", x);
    }
}
