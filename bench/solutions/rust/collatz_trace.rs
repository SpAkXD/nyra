fn main() {
    let mut n: u64 = 6;
    let mut parts = vec![n.to_string()];
    while n != 1 {
        n = if n % 2 == 0 { n / 2 } else { 3 * n + 1 };
        parts.push(n.to_string());
    }
    println!("{}", parts.join(" -> "));
}
