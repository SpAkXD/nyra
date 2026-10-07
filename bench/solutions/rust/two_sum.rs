fn main() {
    let numbers = [4, 7, 1, 9, 6, 5, 2, 8];
    let target = 11;
    for i in 0..numbers.len() {
        for j in (i + 1)..numbers.len() {
            if numbers[i] + numbers[j] == target {
                println!("{} {}", i, j);
            }
        }
    }
}
