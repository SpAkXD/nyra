fn is_prime(n: u32) -> bool {
    if n < 2 {
        return false;
    }
    let mut d = 2;
    while d * d <= n {
        if n % d == 0 {
            return false;
        }
        d += 1;
    }
    true
}

fn main() {
    for n in 2..=100 {
        if is_prime(n) {
            println!("{}", n);
        }
    }
}
