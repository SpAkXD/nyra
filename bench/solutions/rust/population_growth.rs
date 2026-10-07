fn main() {
    let mut population: u32 = 1000;
    for _ in 0..10 {
        population += population * 10 / 100;
        population -= 50;
        println!("{}", population);
    }
}
