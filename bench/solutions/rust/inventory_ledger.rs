use std::collections::BTreeMap;

const COMMANDS: &str = "receive bolt 12; reserve bolt 9; reserve nut 1; receive nut 4; reserve bolt 4; \
receive bolt 10; ship bolt 11; release bolt 1; reserve bolt 7; count bolt 6; count bolt 15; release bolt 7; \
ship nut 4; ship nut 1; receive washer 30; reserve washer 25; ship washer 1; count washer 26; count nut 0; \
ship bolt 15; receive bolt 5; release gear 2";

fn main() {
    let mut items: BTreeMap<String, (i64, i64)> = BTreeMap::new();
    for command in COMMANDS.split("; ") {
        let w: Vec<&str> = command.split(' ').collect();
        let name = w[1];
        let n: i64 = w[2].parse().unwrap();
        if w[0] == "receive" {
            let e = items.entry(name.to_string()).or_insert((0, 0));
            let before = e.0 - e.1;
            e.0 += n;
            println!("{} on-hand {} available {}", name, e.0, e.0 - e.1);
            if e.0 - e.1 < 5 && before >= 5 {
                println!("{} low stock", name);
            }
            continue;
        }
        let e = match items.get_mut(name) {
            None => {
                println!("{} unknown", name);
                continue;
            }
            Some(e) => e,
        };
        let before = e.0 - e.1;
        let ok;
        match w[0] {
            "reserve" => {
                if n > e.0 - e.1 {
                    println!("{} reserve rejected (available {})", name, e.0 - e.1);
                    ok = false;
                } else {
                    e.1 += n;
                    println!("{} reserved {} available {}", name, e.1, e.0 - e.1);
                    ok = true;
                }
            }
            "release" => {
                if n > e.1 {
                    println!("{} release rejected (reserved {})", name, e.1);
                    ok = false;
                } else {
                    e.1 -= n;
                    println!("{} reserved {} available {}", name, e.1, e.0 - e.1);
                    ok = true;
                }
            }
            "ship" => {
                if n > e.0 {
                    println!("{} ship rejected (on-hand {})", name, e.0);
                    ok = false;
                } else {
                    e.0 -= n;
                    e.1 = (e.1 - n).max(0);
                    println!("{} shipped {} on-hand {} reserved {}", name, n, e.0, e.1);
                    ok = true;
                }
            }
            _ => {
                if n < e.1 {
                    println!("{} count rejected (reserved {})", name, e.1);
                    ok = false;
                } else {
                    let d = n - e.0;
                    e.0 = n;
                    if d > 0 {
                        println!("{} adjusted by +{}", name, d);
                    } else {
                        println!("{} adjusted by {}", name, d);
                    }
                    ok = true;
                }
            }
        }
        if ok && e.0 - e.1 < 5 && before >= 5 {
            println!("{} low stock", name);
        }
    }
    println!("---");
    for (name, (h, r)) in &items {
        println!("{}: on-hand {}, reserved {}, available {}", name, h, r, h - r);
    }
}
