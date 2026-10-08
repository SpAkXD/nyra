use std::collections::BTreeMap;

const COMMANDS: &str = "add rent 1 900; add food 1 250; add food 2 -40; move rent 1 food 2 100; \
move food 2 rent 3 200; add fun 12 75; clear fun 12; clear fun 12; add rent 3 1300; move rent 3 rent 3 50; \
add zoo 5 0; add tax 4 -1300; move rent 3 tax 4 1300; add food 1 -250; add car 7 15; move car 7 car 8 15; \
add tax 10 -75; add food 10 12000; move food 10 rent 10 2500";

fn main() {
    let mut ledger: BTreeMap<String, [i64; 13]> = BTreeMap::new();
    for command in COMMANDS.split("; ") {
        let w: Vec<&str> = command.split(' ').collect();
        match w[0] {
            "add" => {
                let m: usize = w[2].parse().unwrap();
                let n: i64 = w[3].parse().unwrap();
                let row = ledger.entry(w[1].to_string()).or_insert([0; 13]);
                row[m] += n;
                println!("{} {} = {}", w[1], m, row[m]);
            }
            "move" => {
                let m1: usize = w[2].parse().unwrap();
                let m2: usize = w[4].parse().unwrap();
                let n: i64 = w[5].parse().unwrap();
                let v1 = ledger.get(w[1]).map(|r| r[m1]).unwrap_or(0);
                if v1 < n {
                    println!("move rejected");
                    continue;
                }
                ledger.entry(w[1].to_string()).or_insert([0; 13])[m1] -= n;
                ledger.entry(w[3].to_string()).or_insert([0; 13])[m2] += n;
                println!(
                    "{} {} = {}, {} {} = {}",
                    w[1],
                    m1,
                    ledger[w[1]][m1],
                    w[3],
                    m2,
                    ledger[w[3]][m2]
                );
            }
            _ => {
                let m: usize = w[2].parse().unwrap();
                let row = ledger.entry(w[1].to_string()).or_insert([0; 13]);
                if row[m] == 0 {
                    println!("{} {} already empty", w[1], m);
                } else {
                    println!("{} {} cleared (was {})", w[1], m, row[m]);
                    row[m] = 0;
                }
            }
        }
    }

    let accounts: Vec<(&String, &[i64; 13])> =
        ledger.iter().filter(|(_, r)| r[1..].iter().any(|v| *v != 0)).collect();
    let months: Vec<usize> = (1..=12).filter(|&m| accounts.iter().any(|(_, r)| r[m] != 0)).collect();

    let mut table: Vec<Vec<String>> = Vec::new();
    let mut header = vec!["account".to_string()];
    header.extend(months.iter().map(|m| m.to_string()));
    header.push("total".to_string());
    table.push(header);
    for (name, r) in &accounts {
        let mut row = vec![name.to_string()];
        for &m in &months {
            row.push(if r[m] == 0 { "-".to_string() } else { r[m].to_string() });
        }
        row.push(r[1..].iter().sum::<i64>().to_string());
        table.push(row);
    }
    let mut total = vec!["total".to_string()];
    for &m in &months {
        total.push(accounts.iter().map(|(_, r)| r[m]).sum::<i64>().to_string());
    }
    total.push(accounts.iter().map(|(_, r)| r[1..].iter().sum::<i64>()).sum::<i64>().to_string());
    table.push(total);

    let cols = table[0].len();
    let widths: Vec<usize> = (0..cols).map(|j| table.iter().map(|r| r[j].len()).max().unwrap()).collect();
    for row in &table {
        let cells: Vec<String> = row
            .iter()
            .enumerate()
            .map(|(j, s)| {
                if j == 0 {
                    format!("{:<w$}", s, w = widths[j])
                } else {
                    format!("{:>w$}", s, w = widths[j])
                }
            })
            .collect();
        println!("{}", cells.join("  ").trim_end());
    }
}
