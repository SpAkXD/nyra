fn main() {
    for k in 1..=6 {
        let row: Vec<String> = (1..=k).map(|i| i.to_string()).collect();
        println!("{}", row.join(" "));
    }
}
