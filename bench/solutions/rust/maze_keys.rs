use std::collections::{HashMap, VecDeque};

const MAZES: [&[&str]; 6] = [
    &["S...#", ".##.#", "....G"],
    &["#########", "#S..A..G#", "#.#####.#", "#...a.#.#", "#########"],
    &["S.B.G", "#.###", "b.A.a"],
    &["#######", "#a#.#b#", "#.#.#.#", "#..S..#", "###A###", "###B###", "###G###"],
    &["S.#..", ".##.#", "...#G"],
    &["..S..", ".###.", "..G.."],
];

// Moves in alphabetical order of their letters.
const MOVES: [(char, i32, i32); 4] = [('D', 1, 0), ('L', 0, -1), ('R', 0, 1), ('U', -1, 0)];

fn solve(rows: &[&str]) -> Option<String> {
    let grid: Vec<Vec<u8>> = rows.iter().map(|r| r.bytes().collect()).collect();
    let h = grid.len() as i32;
    let mut start = (0, 0);
    for (r, row) in grid.iter().enumerate() {
        for (c, &ch) in row.iter().enumerate() {
            if ch == b'S' {
                start = (r as i32, c as i32);
            }
        }
    }
    // Breadth-first search over (row, col, keys); expanding moves in alphabetical order makes the
    // first path found to any state the alphabetically first among the shortest ones.
    let mut seen: HashMap<(i32, i32, u32), String> = HashMap::new();
    let mut queue = VecDeque::new();
    seen.insert((start.0, start.1, 0), String::new());
    queue.push_back((start.0, start.1, 0u32));
    while let Some((r, c, keys)) = queue.pop_front() {
        let path = seen[&(r, c, keys)].clone();
        for &(m, dr, dc) in MOVES.iter() {
            let (nr, nc) = (r + dr, c + dc);
            if nr < 0 || nr >= h || nc < 0 || nc >= grid[nr as usize].len() as i32 {
                continue;
            }
            let ch = grid[nr as usize][nc as usize];
            let mut nk = keys;
            if ch == b'#' {
                continue;
            }
            if ch.is_ascii_uppercase() && ch != b'S' && ch != b'G' && keys & (1 << (ch - b'A')) == 0 {
                continue;
            }
            if ch.is_ascii_lowercase() {
                nk |= 1 << (ch - b'a');
            }
            let mut np = path.clone();
            np.push(m);
            if ch == b'G' {
                return Some(np);
            }
            if !seen.contains_key(&(nr, nc, nk)) {
                seen.insert((nr, nc, nk), np);
                queue.push_back((nr, nc, nk));
            }
        }
    }
    None
}

fn main() {
    for (i, maze) in MAZES.iter().enumerate() {
        match solve(maze) {
            Some(p) => println!("maze {}: {} moves {}", i + 1, p.len(), p),
            None => println!("maze {}: no path", i + 1),
        }
    }
}
