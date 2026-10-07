fn main() {
    let mut count = 0;
    for a in 1..=50 {
        for b in (a + 1)..=50 {
            if (a + b) % 5 == 0 {
                count += 1;
            }
        }
    }
    println!("{}", count);
}
