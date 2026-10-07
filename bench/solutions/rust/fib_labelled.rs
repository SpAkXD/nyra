fn main() {
    let (mut a, mut b): (u64, u64) = (0, 1);
    for n in 0..10 {
        println!("fib({}) = {}", n, a);
        let next = a + b;
        a = b;
        b = next;
    }
}
