fn main() {
    for n in 1..=40 {
        if n.to_string().contains('7') {
            println!("Seven");
        } else if n % 3 == 0 && n % 4 == 0 {
            println!("Twelve");
        } else if n % 3 == 0 {
            println!("Three");
        } else if n % 4 == 0 {
            println!("Four");
        } else {
            println!("{}", n);
        }
    }
}
