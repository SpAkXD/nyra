const EVENTS: &str = "1 borrow ann B1; 1 borrow bob B1; 2 hold bob B1; 2 hold cat B1; 3 hold bob B1; 3 hold ann B1; \
4 hold cat B2; 5 borrow ann B2; 6 borrow ann B3; 6 borrow bob B3; 15 return ann B1; 16 borrow ann B1; 17 borrow bob B1; \
18 hold ann B1; 20 return ann B2; 45 return bob B3; 46 borrow bob B2; 47 pay bob 250; 47 borrow bob B2; 48 return cat B1; \
50 return bob B1; 51 hold cat B1; 52 borrow ann B1; 53 borrow cat B1; 54 pay ann 100; 55 hold ann B1; 56 return cat B1; \
57 borrow ann B1; 58 pay bob 700";

const MEMBERS: [&str; 3] = ["ann", "bob", "cat"];
const BOOKS: [&str; 3] = ["B1", "B2", "B3"];

fn money(c: i64) -> String {
    format!("{}.{:02}", c / 100, c % 100)
}

fn main() {
    let mi = |m: &str| MEMBERS.iter().position(|&x| x == m).unwrap();
    let bi = |b: &str| BOOKS.iter().position(|&x| x == b).unwrap();
    let mut balance = [0i64; 3];
    let mut loan: [Option<(usize, i64)>; 3] = [None; 3]; // (member, due day)
    let mut held: [Option<usize>; 3] = [None; 3];
    let mut waiting: Vec<Vec<usize>> = vec![Vec::new(); 3];
    for ev in EVENTS.split("; ") {
        let w: Vec<&str> = ev.split(' ').collect();
        let day: i64 = w[0].parse().unwrap();
        let m = mi(w[2]);
        let mn = w[2];
        match w[1] {
            "borrow" => {
                let b = bi(w[3]);
                let count = loan.iter().filter(|l| matches!(l, Some((x, _)) if *x == m)).count();
                if balance[m] >= 300 {
                    println!("{} blocked (owes {})", mn, money(balance[m]));
                } else if count >= 2 {
                    println!("{} at limit", mn);
                } else if let Some((_, due)) = loan[b] {
                    println!("{} on loan until day {}", w[3], due);
                } else if matches!(held[b], Some(h) if h != m) {
                    println!("{} held for {}", w[3], MEMBERS[held[b].unwrap()]);
                } else {
                    loan[b] = Some((m, day + 14));
                    held[b] = None;
                    println!("{} borrowed {}, due day {}", mn, w[3], day + 14);
                }
            }
            "return" => {
                let b = bi(w[3]);
                match loan[b] {
                    Some((x, due)) if x == m => {
                        loan[b] = None;
                        if day > due {
                            let late = day - due;
                            let fee = (25 * late).min(500);
                            balance[m] += fee;
                            println!("{} returned {}, {} days late, fee {}", mn, w[3], late, money(fee));
                        } else {
                            println!("{} returned {}", mn, w[3]);
                        }
                        if !waiting[b].is_empty() {
                            let h = waiting[b].remove(0);
                            held[b] = Some(h);
                            println!("{} held for {}", w[3], MEMBERS[h]);
                        }
                    }
                    _ => println!("{} does not have {}", mn, w[3]),
                }
            }
            "hold" => {
                let b = bi(w[3]);
                if matches!(loan[b], Some((x, _)) if x == m) {
                    println!("{} already has {}", mn, w[3]);
                } else if loan[b].is_none() && held[b].is_none() {
                    println!("{} is available", w[3]);
                } else if waiting[b].contains(&m) || held[b] == Some(m) {
                    println!("{} already waiting for {}", mn, w[3]);
                } else {
                    waiting[b].push(m);
                    println!("{} waiting for {}, position {}", mn, w[3], waiting[b].len());
                }
            }
            _ => {
                let x: i64 = w[3].parse().unwrap();
                if x > balance[m] {
                    let y = balance[m];
                    balance[m] = 0;
                    println!("{} paid {}, change {}", mn, money(y), money(x - y));
                } else {
                    balance[m] -= x;
                    println!("{} paid {}, owes {}", mn, money(x), money(balance[m]));
                }
            }
        }
    }
    for (m, name) in MEMBERS.iter().enumerate() {
        let list: Vec<&str> = (0..3)
            .filter(|&b| matches!(loan[b], Some((x, _)) if x == m))
            .map(|b| BOOKS[b])
            .collect();
        let list = if list.is_empty() { "none".to_string() } else { list.join(", ") };
        println!("{} owes {}, has {}", name, money(balance[m]), list);
    }
}
