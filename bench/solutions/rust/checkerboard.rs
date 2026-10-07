fn main() {
    for row in 0..6 {
        let mut line = String::new();
        for col in 0..6 {
            line.push(if (row + col) % 2 == 0 { '#' } else { '.' });
        }
        println!("{}", line);
    }
}
