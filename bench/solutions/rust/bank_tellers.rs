const CUSTOMERS: [(&str, i64, i64, bool); 14] = [
    ("ann", 0, 5, false),
    ("bob", 0, 3, false),
    ("cy", 1, 4, true),
    ("dee", 2, 6, false),
    ("eli", 2, 2, true),
    ("fay", 3, 4, false),
    ("gus", 5, 1, false),
    ("hal", 18, 9, false),
    ("ivy", 19, 3, true),
    ("jon", 20, 4, false),
    ("kim", 20, 5, true),
    ("lou", 22, 4, false),
    ("max", 27, 1, false),
    ("ned", 30, 2, true),
];

fn main() {
    let n = CUSTOMERS.len();
    let mut busy_until: [Option<i64>; 3] = [None; 3];
    let mut line: Vec<usize> = Vec::new();
    let mut waits = vec![0i64; n];
    let mut served = 0;
    let mut t = 0i64;
    while served < n {
        for b in busy_until.iter_mut() {
            if *b == Some(t) {
                *b = None;
            }
        }
        for (i, c) in CUSTOMERS.iter().enumerate() {
            if c.1 == t {
                if c.3 {
                    let pos = line.iter().position(|&j| !CUSTOMERS[j].3).unwrap_or(line.len());
                    line.insert(pos, i);
                } else {
                    line.push(i);
                }
            }
        }
        while !line.is_empty() {
            let teller = (0..3).find(|&k| busy_until[k].is_none() && !(k == 2 && (20..=29).contains(&t)));
            let k = match teller {
                Some(k) => k,
                None => break,
            };
            let i = line.remove(0);
            let c = CUSTOMERS[i];
            let end = t + c.2;
            busy_until[k] = Some(end);
            waits[i] = t - c.1;
            println!("{} {} teller {} wait {} end {}", t, c.0, k + 1, waits[i], end);
            served += 1;
        }
        t += 1;
    }
    let total: i64 = waits.iter().sum();
    let mut best = 0;
    for i in 1..n {
        if waits[i] > waits[best] {
            best = i;
        }
    }
    println!("total wait {}, longest wait {} ({})", total, waits[best], CUSTOMERS[best].0);
}
