fn steps(mut n: u64) -> u32 {
    let mut count = 0;
    while n != 1 {
        n = if n % 2 == 0 { n / 2 } else { 3 * n + 1 };
        count += 1;
    }
    count
}

fn main() {
    let mut best = 1;
    let mut best_steps = 0;
    for start in 1..10000 {
        let s = steps(start);
        if s > best_steps {
            best = start;
            best_steps = s;
        }
    }
    println!("{}", best);
    println!("{}", best_steps);
}
