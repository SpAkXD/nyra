fn fib(n: u32) -> u64 {
    if n <= 2 {
        1
    } else {
        fib(n - 1) + fib(n - 2)
    }
}

fn main() {
    for n in [10, 20, 25] {
        println!("{}", fib(n));
    }
}
