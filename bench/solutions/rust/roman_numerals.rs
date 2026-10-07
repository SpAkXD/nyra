const SYMBOLS: [(u32, &str); 13] = [
    (1000, "M"), (900, "CM"), (500, "D"), (400, "CD"), (100, "C"), (90, "XC"),
    (50, "L"), (40, "XL"), (10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I"),
];

fn to_roman(mut n: u32) -> String {
    let mut result = String::new();
    for (value, symbol) in SYMBOLS {
        while n >= value {
            result.push_str(symbol);
            n -= value;
        }
    }
    result
}

fn main() {
    for n in [4, 9, 14, 40, 90, 400, 1994, 2024] {
        println!("{}", to_roman(n));
    }
}
