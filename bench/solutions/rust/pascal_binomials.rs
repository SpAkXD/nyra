fn binom(n: u32, k: u32) -> u64 {
    if k == 0 || k == n {
        1
    } else {
        binom(n - 1, k - 1) + binom(n - 1, k)
    }
}

fn main() {
    for (n, k) in [(5, 2), (10, 5), (20, 10)] {
        println!("{}", binom(n, k));
    }
}
