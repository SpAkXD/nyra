fn main() {
    let n: usize = 500_000;
    let m: u64 = 1_000_000_007;
    let mut xs: Vec<u64> = Vec::with_capacity(n);
    let mut x: u64 = 12345;
    for _ in 0..n {
        x = x * 48271 % 2147483647;
        xs.push(x);
    }
    xs.sort();
    let mut check: u64 = 0;
    for (i, v) in xs.iter().enumerate() {
        check = (check + (i as u64 + 1) * v) % m;
    }
    println!("{}", xs[0]);
    println!("{}", xs[n - 1]);
    println!("{}", xs[n / 2]);
    println!("{}", check);
}
