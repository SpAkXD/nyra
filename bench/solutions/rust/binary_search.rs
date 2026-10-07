fn search(values: &[i32], target: i32) -> i32 {
    let mut low: i32 = 0;
    let mut high: i32 = values.len() as i32 - 1;
    while low <= high {
        let mid = (low + high) / 2;
        let value = values[mid as usize];
        if value == target {
            return mid;
        }
        if value < target {
            low = mid + 1;
        } else {
            high = mid - 1;
        }
    }
    -1
}

fn main() {
    let values = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37];
    for target in [23, 2, 37, 4] {
        println!("{}", search(&values, target));
    }
}
