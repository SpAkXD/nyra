fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

fn main() {
    let pairs = [(48, 18), (1071, 462), (17, 5), (1000000, 250000), (270, 192)];
    for (a, b) in pairs {
        println!("{}", gcd(a, b));
    }
}
