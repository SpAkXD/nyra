const PEOPLE: [&str; 4] = ["ann", "bob", "cat", "dan"];
const BUSY: [&str; 4] = [
    "09:00-10:30, 12:00-13:00, 15:45-17:00",
    "09:30-11:15, 13:00-14:00",
    "11:00-12:15, 14:30-15:15",
    "10:00-10:45, 13:30-16:00",
];
const REQUESTS: &str = "ann+bob 60; bob+cat+dan 30; ann+cat 45; ann+bob+cat+dan 30; dan 120; cat+dan 15; ann+bob 45; bob 120";
const DAY: (i32, i32) = (9 * 60, 17 * 60);

fn parse_time(s: &str) -> i32 {
    let (h, m) = s.split_once(':').unwrap();
    h.parse::<i32>().unwrap() * 60 + m.parse::<i32>().unwrap()
}

fn fmt(t: i32) -> String {
    format!("{:02}:{:02}", t / 60, t % 60)
}

fn main() {
    let mut busy: Vec<Vec<(i32, i32)>> = BUSY
        .iter()
        .map(|line| {
            line.split(", ")
                .map(|p| {
                    let (a, b) = p.split_once('-').unwrap();
                    (parse_time(a), parse_time(b))
                })
                .collect()
        })
        .collect();
    for req in REQUESTS.split("; ") {
        let (names, dur) = req.split_once(' ').unwrap();
        let dur: i32 = dur.parse().unwrap();
        let who: Vec<usize> = names.split('+').map(|n| PEOPLE.iter().position(|p| *p == n).unwrap()).collect();
        let mut found = None;
        let mut s = DAY.0;
        while s + dur <= DAY.1 {
            let e = s + dur;
            if who.iter().all(|&w| busy[w].iter().all(|&(a, b)| !(s < b && a < e))) {
                found = Some((s, e));
                break;
            }
            s += 15;
        }
        match found {
            Some((s, e)) => {
                for &w in &who {
                    busy[w].push((s, e));
                }
                println!("{}: {}-{}", names, fmt(s), fmt(e));
            }
            None => println!("{}: no slot", names),
        }
    }
    for (i, name) in PEOPLE.iter().enumerate() {
        let mut b = busy[i].clone();
        b.sort();
        let mut free: Vec<String> = Vec::new();
        let mut t = DAY.0;
        for &(a, e) in &b {
            if a > t {
                free.push(format!("{}-{}", fmt(t), fmt(a)));
            }
            t = t.max(e);
        }
        if t < DAY.1 {
            free.push(format!("{}-{}", fmt(t), fmt(DAY.1)));
        }
        let text = if free.is_empty() { "none".to_string() } else { free.join(", ") };
        println!("{} free: {}", name, text);
    }
}
