const ROWS: usize = 6;
const COLS: usize = 8;

fn main() {
    let start = ["#......#", "..##....", ".#..#...", "..##....", "........", "#.....##"];
    let mut grid: Vec<Vec<u32>> = start
        .iter()
        .map(|r| r.chars().map(|c| if c == '#' { 1 } else { 0 }).collect())
        .collect();
    for g in 1..=7 {
        let mut next = vec![vec![0u32; COLS]; ROWS];
        for r in 0..ROWS {
            for c in 0..COLS {
                let mut live = 0;
                for dr in 0..3 {
                    for dc in 0..3 {
                        if dr == 1 && dc == 1 {
                            continue;
                        }
                        let rr = (r + ROWS + dr - 1) % ROWS;
                        let cc = (c + COLS + dc - 1) % COLS;
                        if grid[rr][cc] > 0 {
                            live += 1;
                        }
                    }
                }
                let age = grid[r][c];
                next[r][c] = if age == 0 {
                    if live == 3 { 1 } else { 0 }
                } else if (live == 2 || live == 3) && age + 1 != 5 {
                    age + 1
                } else {
                    0
                };
            }
        }
        grid = next;
        let alive = grid.iter().flatten().filter(|a| **a > 0).count();
        let oldest = grid.iter().flatten().copied().max().unwrap();
        println!("gen {}: {} alive, oldest {}", g, alive, oldest);
    }
    for row in &grid {
        let s: String = row
            .iter()
            .map(|a| if *a == 0 { '.' } else { std::char::from_digit(*a, 10).unwrap() })
            .collect();
        println!("{}", s);
    }
}
