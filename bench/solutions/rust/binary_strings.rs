fn to_binary(mut n: u32) -> String {
    let mut digits = String::new();
    while n > 0 {
        digits.insert(0, if n % 2 == 1 { '1' } else { '0' });
        n /= 2;
    }
    digits
}

fn main() {
    for n in [5, 10, 255, 1024] {
        println!("{}", to_binary(n));
    }
}
