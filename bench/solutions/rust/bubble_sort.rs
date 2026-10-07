fn main() {
    let mut numbers = [64, 34, 25, 12, 22, 11, 90];
    let n = numbers.len();
    for i in 0..n {
        for j in 0..n - 1 - i {
            if numbers[j] > numbers[j + 1] {
                numbers.swap(j, j + 1);
            }
        }
    }
    for value in numbers {
        println!("{}", value);
    }
}
