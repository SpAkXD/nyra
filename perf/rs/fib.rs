// recursion: fib(n) for n = 0 to 35, summed (reference for fib.nyra)
fn fib(n: i64) -> i64 {
    if n < 2 {
        return n;
    }
    fib(n - 1) + fib(n - 2)
}

fn main() {
    let mut total = 0;
    for n in 0..36 {
        total += fib(n);
    }
    println!("{total}");
}
