fn reverse(mut n: u64) -> u64 {
    let mut result = 0;
    while n > 0 {
        result = result * 10 + n % 10;
        n /= 10;
    }
    result
}

fn main() {
    for n in [12345, 1200, 907, 86420] {
        println!("{}", reverse(n));
    }
}
