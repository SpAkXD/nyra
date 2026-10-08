const MONTHS: [&str; 6] = [
    "deposit 1200.00; withdraw 1500.00; withdraw 197.50",
    "deposit 4000.50; withdraw 20.00",
    "withdraw 3978.75; deposit 6000.00",
    "withdraw 7500.00; withdraw 1997.76",
    "withdraw 5011.26",
    "withdraw 2.00; deposit 0.50",
];

fn cents(s: &str) -> i64 {
    let (a, b) = s.split_once('.').unwrap();
    a.parse::<i64>().unwrap() * 100 + b.parse::<i64>().unwrap()
}

fn money(c: i64) -> String {
    let sign = if c < 0 { "-" } else { "" };
    let a = c.abs();
    format!("{}{}.{:02}", sign, a / 100, a % 100)
}

// round num/den (num >= 0, den > 0) to nearest, ties to even
fn round_half_even(num: i64, den: i64) -> i64 {
    let q = num / den;
    let r = num % den;
    if 2 * r > den || (2 * r == den && q % 2 == 1) {
        q + 1
    } else {
        q
    }
}

fn main() {
    let mut balance: i64 = 0;
    let mut total_interest = 0;
    let mut total_fees = 0;
    for (idx, line) in MONTHS.iter().enumerate() {
        let month = idx + 1;
        let mut min = balance;
        for tx in line.split("; ") {
            let (kind, amount) = tx.split_once(' ').unwrap();
            let x = cents(amount);
            if kind == "deposit" {
                balance += x;
            } else if balance - x < 0 {
                println!("month {}: withdraw {} rejected", month, amount);
            } else {
                balance -= x;
            }
            min = min.min(balance);
        }
        let p1 = min.min(100_000).max(0);
        let p2 = (min.min(500_000) - 100_000).max(0);
        let p3 = (min - 500_000).max(0);
        // yearly interest in cents = (p1*15 + p2*24 + p3*30) / 1000; monthly = that / 12
        let interest = round_half_even(p1 * 15 + p2 * 24 + p3 * 30, 12_000);
        balance += interest;
        let mut fee = 0;
        if min < 50_000 {
            fee = if balance < 300 { balance } else { 300 };
            balance -= fee;
        }
        total_interest += interest;
        total_fees += fee;
        println!(
            "month {}: min {} interest {} fee {} balance {}",
            month,
            money(min),
            money(interest),
            money(fee),
            money(balance)
        );
    }
    println!("total interest {} fees {}", money(total_interest), money(total_fees));
}
