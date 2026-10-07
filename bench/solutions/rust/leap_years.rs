fn is_leap(year: u32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn main() {
    for year in [1900, 1996, 2000, 2023, 2024, 2100] {
        println!("{}", if is_leap(year) { "yes" } else { "no" });
    }
}
