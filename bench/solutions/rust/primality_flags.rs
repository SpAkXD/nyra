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
    for n in [1, 2, 3, 4, 17, 25, 97, 100, 7919] {
        println!("{}", if is_prime(n) { "true" } else { "false" });
    }
}
