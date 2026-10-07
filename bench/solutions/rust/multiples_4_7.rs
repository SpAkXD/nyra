fn main() {
    let sum: u32 = (1..1000).filter(|n| n % 4 == 0 || n % 7 == 0).sum();
    println!("{}", sum);
}
