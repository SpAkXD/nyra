const COMMANDS: &str = "open A 100; open B 50; open A 10; deposit C 5; withdraw B 70; transfer A B 60; \
transfer B A 112; transfer B A 109; transfer A A 5; withdraw A 25; close B; transfer A B 1; deposit A 7; \
withdraw A 15; close A; open B 0; close B";

fn slot(name: &str) -> usize {
    (name.as_bytes()[0] - b'A') as usize
}

fn main() {
    let mut accounts: [Option<i64>; 26] = [None; 26];
    for command in COMMANDS.split("; ") {
        let words: Vec<&str> = command.split(' ').collect();
        let x = words[1];
        match words[0] {
            "open" => match accounts[slot(x)] {
                Some(_) => println!("{} exists", x),
                None => {
                    accounts[slot(x)] = Some(words[2].parse().unwrap());
                    println!("opened {}", x);
                }
            },
            "deposit" => match accounts[slot(x)].as_mut() {
                None => println!("{} unknown", x),
                Some(balance) => {
                    *balance += words[2].parse::<i64>().unwrap();
                    println!("{} balance {}", x, balance);
                }
            },
            "withdraw" => {
                let n: i64 = words[2].parse().unwrap();
                match accounts[slot(x)].as_mut() {
                    None => println!("{} unknown", x),
                    Some(balance) if *balance - n < 0 => println!("{} insufficient", x),
                    Some(balance) => {
                        *balance -= n;
                        println!("{} balance {}", x, balance);
                    }
                }
            }
            "transfer" => {
                let y = words[2];
                let n: i64 = words[3].parse().unwrap();
                match (accounts[slot(x)], accounts[slot(y)]) {
                    (Some(from), Some(_)) => {
                        if from < n + 1 {
                            println!("{} insufficient", x);
                        } else {
                            // pay the fee, then move n (which changes nothing when both are the same account)
                            accounts[slot(x)] = Some(from - 1 - n);
                            accounts[slot(y)] = Some(accounts[slot(y)].unwrap() + n);
                            println!("{} balance {} {} balance {}", x, accounts[slot(x)].unwrap(), y,
                                     accounts[slot(y)].unwrap());
                        }
                    }
                    _ => println!("unknown account"),
                }
            }
            "close" => match accounts[slot(x)] {
                None => println!("{} unknown", x),
                Some(balance) if balance != 0 => println!("{} not empty", x),
                Some(_) => {
                    accounts[slot(x)] = None;
                    println!("closed {}", x);
                }
            },
            other => panic!("unknown command {}", other),
        }
    }
}
