// the ways either use no coin of the largest kind, or use one and make the rest with the same kinds
fn ways(amount: i32, coins: &[i32]) -> u64 {
    if amount == 0 {
        return 1;
    }
    if amount < 0 || coins.is_empty() {
        return 0;
    }
    let (largest, others) = coins.split_last().unwrap();
    ways(amount, others) + ways(amount - largest, coins)
}

fn main() {
    println!("{}", ways(175, &[1, 5, 10, 25, 50]));
}
