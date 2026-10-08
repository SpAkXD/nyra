const GAMES: [&[i32]; 7] = [
    &[3, 4, 3, 4, 3, 4, 3],
    &[1, 2, 2, 3, 3, 4, 3, 4, 4, 6, 4],
    &[6, 6, 6, 6, 6, 6],
    &[2, 3, 4, 5, 1, 2, 3, 4, 7],
    &[1, 1, 2, 2, 4, 4, 3, 5, 5],
    &[4, 3, 3, 2, 2, 1, 2, 1, 1, 5, 1],
    &[1, 2, 3, 4, 5, 6, 6, 5],
];
const ROWS: usize = 5;
const COLS: usize = 6;

// Does the disc at (r, c) belong to a line of 4 or more? Row 0 is the bottom.
fn wins(b: &[[char; COLS]; ROWS], r: usize, c: usize) -> bool {
    let p = b[r][c];
    for (dr, dc) in [(0i32, 1i32), (1, 0), (1, 1), (1, -1)] {
        let mut n = 1;
        for s in [1i32, -1] {
            let (mut rr, mut cc) = (r as i32 + s * dr, c as i32 + s * dc);
            while rr >= 0 && rr < ROWS as i32 && cc >= 0 && cc < COLS as i32 && b[rr as usize][cc as usize] == p {
                n += 1;
                rr += s * dr;
                cc += s * dc;
            }
        }
        if n >= 4 {
            return true;
        }
    }
    false
}

fn main() {
    for (g, moves) in GAMES.iter().enumerate() {
        let mut b = [['.'; COLS]; ROWS];
        let mut result = None;
        for (i, &m) in moves.iter().enumerate() {
            let p = if i % 2 == 0 { 'X' } else { 'O' };
            let row = if (1..=COLS as i32).contains(&m) {
                (0..ROWS).find(|&r| b[r][(m - 1) as usize] == '.')
            } else {
                None
            };
            let r = match row {
                Some(r) => r,
                None => {
                    result = Some(format!("illegal move {} by {}", i + 1, p));
                    break;
                }
            };
            let c = (m - 1) as usize;
            b[r][c] = p;
            if wins(&b, r, c) {
                result = Some(format!("{} wins at move {}", p, i + 1));
                break;
            }
        }
        let result = result.unwrap_or_else(|| {
            if b.iter().all(|row| row.iter().all(|&x| x != '.')) {
                "draw".to_string()
            } else {
                format!("no winner after {} moves", moves.len())
            }
        });
        println!("game {}: {}", g + 1, result);
        for r in (0..ROWS).rev() {
            println!("{}", b[r].iter().collect::<String>());
        }
    }
}
