fn main() {
    let mut result: u64 = 1;
    for _ in 0..200 {
        result = result * 3 % 1000007;
    }
    println!("{}", result);
}
