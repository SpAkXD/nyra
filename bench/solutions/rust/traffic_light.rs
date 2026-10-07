fn main() {
    for t in 0..15 {
        match t % 6 {
            0..=2 => println!("green"),
            3 => println!("yellow"),
            _ => println!("red"),
        }
    }
}
