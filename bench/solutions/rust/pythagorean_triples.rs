fn main() {
    for c in 1u32..=50 {
        for a in 1..c {
            let rest = c * c - a * a;
            let b = (rest as f64).sqrt().round() as u32;
            if b > a && b * b == rest {
                println!("{} {} {}", a, b, c);
            }
        }
    }
}
