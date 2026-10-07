fn main() {
    let days = 31;
    let first = 2; // Monday is 0, so Wednesday is 2
    let mut grid: Vec<Option<u32>> = vec![None; first];
    grid.extend((1..=days).map(Some));
    println!("Mo Tu We Th Fr Sa Su");
    for week in grid.chunks(7) {
        let line: Vec<String> = week
            .iter()
            .map(|cell| match cell {
                Some(day) => format!("{:>2}", day),
                None => "  ".to_string(),
            })
            .collect();
        println!("{}", line.join(" ").trim_end());
    }
}
