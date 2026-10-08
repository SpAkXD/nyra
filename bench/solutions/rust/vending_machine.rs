const EVENTS: &str = "insert 25; insert 25; insert 3; select A1; insert 25; select A1; insert 100; select B1; \
select C9; select A2; select A2; insert 100; insert 25; select B2; insert 100; select A1; insert 100; select A1; \
cancel; insert 10; insert 10; insert 10; insert 100; select A1; cancel; select B2; insert 50; cancel";

const VALUES: [i64; 4] = [100, 25, 10, 5];

// greedy change: returns coins taken per value and the unpaid remainder
fn make_change(amount: i64, store: &[i64; 4]) -> ([i64; 4], i64) {
    let mut left = amount;
    let mut taken = [0i64; 4];
    for i in 0..4 {
        let k = (left / VALUES[i]).min(store[i]);
        taken[i] = k;
        left -= k * VALUES[i];
    }
    (taken, left)
}

fn list(taken: &[i64; 4]) -> String {
    let parts: Vec<String> = (0..4)
        .filter(|&i| taken[i] > 0)
        .map(|i| format!("{}x{}", VALUES[i], taken[i]))
        .collect();
    if parts.is_empty() { "none".to_string() } else { parts.join(" ") }
}

fn main() {
    let mut slots: Vec<(&str, i64, i64)> = vec![("A1", 65, 3), ("A2", 100, 1), ("B1", 45, 0), ("B2", 120, 2)];
    let mut store: [i64; 4] = [0, 2, 1, 3];
    let mut credit: i64 = 0;
    for event in EVENTS.split("; ") {
        let w: Vec<&str> = event.split(' ').collect();
        match w[0] {
            "insert" => {
                let n: i64 = w[1].parse().unwrap();
                match VALUES.iter().position(|v| *v == n) {
                    None => println!("rejected {}", n),
                    Some(i) => {
                        store[i] += 1;
                        credit += n;
                        println!("credit {}", credit);
                    }
                }
            }
            "select" => {
                let s = w[1];
                match slots.iter().position(|x| x.0 == s) {
                    None => println!("no slot {}", s),
                    Some(i) => {
                        let (_, price, stock) = slots[i];
                        if stock == 0 {
                            println!("{} sold out", s);
                        } else if credit < price {
                            println!("{} costs {}, insert {} more", s, price, price - credit);
                        } else {
                            let (taken, left) = make_change(credit - price, &store);
                            if left > 0 {
                                println!("{} exact change only", s);
                            } else {
                                slots[i].2 -= 1;
                                credit = 0;
                                for k in 0..4 {
                                    store[k] -= taken[k];
                                }
                                println!("{} vended, change {}", s, list(&taken));
                            }
                        }
                    }
                }
            }
            _ => {
                let (taken, left) = make_change(credit, &store);
                for k in 0..4 {
                    store[k] -= taken[k];
                }
                credit = 0;
                if left > 0 {
                    println!("returned {} (owed {})", list(&taken), left);
                } else {
                    println!("returned {}", list(&taken));
                }
            }
        }
    }
    println!("stock A1 {} A2 {} B1 {} B2 {}", slots[0].2, slots[1].2, slots[2].2, slots[3].2);
    println!("coins 100x{} 25x{} 10x{} 5x{}", store[0], store[1], store[2], store[3]);
}
