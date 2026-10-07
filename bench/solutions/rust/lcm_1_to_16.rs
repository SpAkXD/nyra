fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

fn main() {
    let mut result: u64 = 1;
    for n in 2..=16 {
        result = result / gcd(result, n) * n;
    }
    println!("{}", result);
}
