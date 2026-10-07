fn main() {
    for c in (0..=100).step_by(20) {
        println!("{}C = {}F", c, c * 9 / 5 + 32);
    }
}
