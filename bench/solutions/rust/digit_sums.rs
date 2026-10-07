fn digit_sum(mut n: u64) -> u64 {
    let mut total = 0;
    while n > 0 {
        total += n % 10;
        n /= 10;
    }
    total
}

fn main() {
    for n in [12345, 9999, 100000, 987654321] {
        println!("{}", digit_sum(n));
    }
}
