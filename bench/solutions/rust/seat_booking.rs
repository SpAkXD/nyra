const REQUESTS: &str = "ann 4; bob 4; cat 2; dan 10; eve 1; fay 6; cancel bob; gus 5; hal 3; cancel zed; ivy 9; jo 2; kai 8; lea 3";

#[derive(Clone, PartialEq)]
enum Seat {
    Broken,
    Free,
    Booked(String),
}

fn row_index(c: char) -> usize {
    (c as u8 - b'A') as usize
}

fn main() {
    let mut seats: Vec<Vec<Seat>> = vec![vec![Seat::Free; 10]; 6];
    for b in ["C5", "C6", "E1"] {
        let r = row_index(b.chars().next().unwrap());
        let s: usize = b[1..].parse().unwrap();
        seats[r][s - 1] = Seat::Broken;
    }
    for req in REQUESTS.split("; ") {
        let (a, b) = req.split_once(' ').unwrap();
        if a == "cancel" {
            let mut n = 0;
            for row in seats.iter_mut() {
                for seat in row.iter_mut() {
                    if *seat == Seat::Booked(b.to_string()) {
                        *seat = Seat::Free;
                        n += 1;
                    }
                }
            }
            if n == 0 {
                println!("{} has no booking", b);
            } else {
                println!("{} cancelled ({} seats)", b, n);
            }
            continue;
        }
        let name = a;
        let k: usize = b.parse().unwrap();
        let mut done = false;
        for rc in ['C', 'D', 'B', 'E', 'A', 'F'] {
            let r = row_index(rc);
            // best (distance, start) with start 0-based
            let mut best: Option<(i64, usize)> = None;
            if k <= 10 {
                for s in 0..=(10 - k) {
                    if (s..s + k).all(|i| seats[r][i] == Seat::Free) {
                        // seat numbers s+1 .. s+k; doubled middle = 2s + k + 1; row middle doubled = 11
                        let dist = (2 * s as i64 + k as i64 + 1 - 11).abs();
                        if best.map_or(true, |(d, _)| dist < d) {
                            best = Some((dist, s));
                        }
                    }
                }
            }
            if let Some((_, s)) = best {
                for i in s..s + k {
                    seats[r][i] = Seat::Booked(name.to_string());
                }
                if k == 1 {
                    println!("{}: {}{}", name, rc, s + 1);
                } else {
                    println!("{}: {}{}-{}{}", name, rc, s + 1, rc, s + k);
                }
                done = true;
                break;
            }
        }
        if !done {
            println!("{}: no room", name);
        }
    }
    for (r, row) in seats.iter().enumerate() {
        let line: String = row
            .iter()
            .map(|s| match s {
                Seat::Broken => 'x',
                Seat::Free => '.',
                Seat::Booked(_) => '#',
            })
            .collect();
        println!("{} {}", (b'A' + r as u8) as char, line);
    }
}
