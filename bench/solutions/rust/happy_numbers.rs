fn next(mut n: u32) -> u32 {
    let mut sum = 0;
    while n > 0 {
        sum += (n % 10) * (n % 10);
        n /= 10;
    }
    sum
}

// every number that is not happy ends in the cycle 4, 16, 37, 58, 89, 145, 42, 20, 4
fn is_happy(mut n: u32) -> bool {
    while n != 1 && n != 4 {
        n = next(n);
    }
    n == 1
}

fn main() {
    let happy: Vec<u32> = (1..).filter(|&n| is_happy(n)).take(10).collect();
    for n in happy {
        println!("{}", n);
    }
}
