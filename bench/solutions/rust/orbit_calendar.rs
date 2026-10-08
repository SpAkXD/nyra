const QUERIES: &str = "ADD 0001-01-01 0; ADD 0359-10-40 3; ADD 0360-10-41 1; ADD 0450-03-17 -1000; \
DIFF 0090-10-41 0091-01-01; DIFF 0721-05-30 0001-01-01; ADD 0539-10-42 1; NTH 3 Dun 0540-07; NTH 7 Fen 0012-10; \
NTH 7 Ari 0013-10; DIFF 0180-10-42 0181-01-01; ADD 0002-01-01 -365; NTH 7 Bel 0013-10; ADD 0005-11-01 1; \
ADD 0359-01-01 365";

const WEEK: [&str; 6] = ["Ari", "Bel", "Cor", "Dun", "Eld", "Fen"];

fn leap(y: i64) -> bool {
    (y % 6 == 0 && y % 90 != 0) || y % 360 == 0
}

fn month_len(y: i64, m: i64) -> i64 {
    if m < 10 {
        36
    } else if leap(y) {
        42
    } else {
        41
    }
}

fn year_len(y: i64) -> i64 {
    9 * 36 + month_len(y, 10)
}

// day number counted from 0 for 0001-01-01
fn to_days(y: i64, m: i64, d: i64) -> i64 {
    let mut n = 0;
    for yy in 1..y {
        n += year_len(yy);
    }
    for mm in 1..m {
        n += month_len(y, mm);
    }
    n + d - 1
}

fn from_days(mut n: i64) -> (i64, i64, i64) {
    let mut y = 1;
    while n >= year_len(y) {
        n -= year_len(y);
        y += 1;
    }
    let mut m = 1;
    while n >= month_len(y, m) {
        n -= month_len(y, m);
        m += 1;
    }
    (y, m, n + 1)
}

fn fmt(n: i64) -> String {
    let (y, m, d) = from_days(n);
    format!("{:04}-{:02}-{:02} {}", y, m, d, WEEK[(n % 6) as usize])
}

fn parse_date(s: &str) -> Option<i64> {
    let p: Vec<i64> = s.split('-').map(|x| x.parse().unwrap()).collect();
    let (y, m, d) = (p[0], p[1], p[2]);
    if y < 1 || m < 1 || m > 10 || d < 1 || d > month_len(y, m) {
        return None;
    }
    Some(to_days(y, m, d))
}

fn main() {
    for q in QUERIES.split("; ") {
        let w: Vec<&str> = q.split(' ').collect();
        match w[0] {
            "ADD" => match parse_date(w[1]) {
                None => println!("invalid date"),
                Some(n) => println!("{}", fmt(n + w[2].parse::<i64>().unwrap())),
            },
            "DIFF" => match (parse_date(w[1]), parse_date(w[2])) {
                (Some(a), Some(b)) => println!("{}", b - a),
                _ => println!("invalid date"),
            },
            _ => {
                let k: i64 = w[1].parse().unwrap();
                let wd = WEEK.iter().position(|x| *x == w[2]).unwrap() as i64;
                let p: Vec<i64> = w[3].split('-').map(|x| x.parse().unwrap()).collect();
                let (y, m) = (p[0], p[1]);
                if y < 1 || m < 1 || m > 10 {
                    println!("invalid date");
                    continue;
                }
                let first = to_days(y, m, 1);
                let offset = (wd - first % 6 + 6) % 6;
                let day = 1 + offset + 6 * (k - 1);
                if day > month_len(y, m) {
                    println!("none");
                } else {
                    println!("{}", fmt(first + day - 1));
                }
            }
        }
    }
}
