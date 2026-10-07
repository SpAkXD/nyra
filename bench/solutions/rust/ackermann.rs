fn ack(m: u64, n: u64) -> u64 {
    if m == 0 {
        n + 1
    } else if n == 0 {
        ack(m - 1, 1)
    } else {
        ack(m - 1, ack(m, n - 1))
    }
}

fn main() {
    let cases = [(0, 0), (1, 2), (2, 3), (3, 3), (3, 5)];
    for (m, n) in cases {
        println!("{}", ack(m, n));
    }
}
