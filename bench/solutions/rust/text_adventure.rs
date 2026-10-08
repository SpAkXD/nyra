const COMMANDS: &str = "look; go north; unlock east; go east; go south; take lamp; go west; go east; go down; \
take coin; take coin; go up; go north; take key; drop coin; take key; inventory; go west; look; go north; \
unlock west; unlock west; go west; unlock east; drop lamp; go south; go east; go down; look; unlock up; \
take rope; go north; take coin; go south; go west; drop coin; inventory";

fn opposite(d: &str) -> &'static str {
    match d {
        "north" => "south",
        "south" => "north",
        "east" => "west",
        "west" => "east",
        "down" => "up",
        _ => "down",
    }
}

struct Passage {
    from: &'static str,
    dir: &'static str,
    to: &'static str,
    locked: bool,
}

fn main() {
    let base: [(&str, &str, &str); 6] = [
        ("hall", "north", "library"),
        ("hall", "east", "kitchen"),
        ("kitchen", "north", "pantry"),
        ("kitchen", "down", "cellar"),
        ("library", "east", "study"),
        ("study", "south", "pantry"),
    ];
    // passages stored in pairs; index / 2 identifies the shared passage
    let mut passages: Vec<Passage> = Vec::new();
    for &(a, d, b) in base.iter() {
        let locked = a == "library" && b == "study";
        passages.push(Passage { from: a, dir: d, to: b, locked });
        passages.push(Passage { from: b, dir: opposite(d), to: a, locked });
    }
    let mut items: Vec<(String, String)> = vec![
        ("lamp".into(), "hall".into()),
        ("key".into(), "pantry".into()),
        ("rope".into(), "pantry".into()),
        ("book".into(), "study".into()),
        ("coin".into(), "cellar".into()),
    ];
    let mut room: &str = "hall";
    let mut carried: Vec<String> = Vec::new();
    let mut visited: Vec<&str> = vec!["hall"];

    for command in COMMANDS.split("; ") {
        let w: Vec<&str> = command.split(' ').collect();
        let find = |passages: &Vec<Passage>, room: &str, d: &str| -> Option<usize> {
            passages.iter().position(|p| p.from == room && p.dir == d)
        };
        match w[0] {
            "go" => {
                let d = w[1];
                match find(&passages, room, d) {
                    None => println!("no exit {}", d),
                    Some(i) => {
                        let p = &passages[i];
                        if p.locked {
                            println!("the door is locked");
                        } else if p.to == "cellar" && !carried.iter().any(|c| c == "lamp") {
                            println!("too dark to enter");
                        } else {
                            room = p.to;
                            if !visited.contains(&room) {
                                visited.push(room);
                            }
                            println!("you are in {}", room);
                        }
                    }
                }
            }
            "take" => {
                let x = w[1];
                match items.iter().position(|(n, r)| n == x && r == room) {
                    None => println!("no {} here", x),
                    Some(i) => {
                        if carried.len() >= 2 {
                            println!("hands full");
                        } else {
                            items.remove(i);
                            carried.push(x.to_string());
                            println!("taken {}", x);
                        }
                    }
                }
            }
            "drop" => {
                let x = w[1];
                match carried.iter().position(|c| c == x) {
                    None => println!("you have no {}", x),
                    Some(i) => {
                        carried.remove(i);
                        items.push((x.to_string(), room.to_string()));
                        println!("dropped {}", x);
                    }
                }
            }
            "unlock" => {
                let d = w[1];
                match find(&passages, room, d) {
                    None => println!("no exit {}", d),
                    Some(i) => {
                        if !passages[i].locked {
                            println!("nothing to unlock");
                        } else if !carried.iter().any(|c| c == "key") {
                            println!("you need the key");
                        } else {
                            let pair = i / 2 * 2;
                            passages[pair].locked = false;
                            passages[pair + 1].locked = false;
                            println!("unlocked");
                        }
                    }
                }
            }
            "look" => {
                let mut here: Vec<&str> = items
                    .iter()
                    .filter(|(_, r)| r == room)
                    .map(|(n, _)| n.as_str())
                    .collect();
                here.sort();
                let mut exits: Vec<&str> = passages
                    .iter()
                    .filter(|p| p.from == room)
                    .map(|p| p.dir)
                    .collect();
                exits.sort();
                let i = if here.is_empty() { "none".to_string() } else { here.join(", ") };
                println!("{} | items: {} | exits: {}", room, i, exits.join(", "));
            }
            _ => {
                let mut c: Vec<&str> = carried.iter().map(|s| s.as_str()).collect();
                c.sort();
                if c.is_empty() {
                    println!("carrying: nothing");
                } else {
                    println!("carrying: {}", c.join(", "));
                }
            }
        }
    }
    let mut score = 10 * (visited.len() as i64 - 1);
    if items.iter().any(|(n, r)| n == "coin" && r == "hall") {
        score += 25;
    }
    println!("score {}", score);
}
