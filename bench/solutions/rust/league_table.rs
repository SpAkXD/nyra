const RESULTS: &str = "Ash 2-1 Bay; Cove 2-0 Ash; Dale 2-2 Ash; Ash 0-2 Elm; Fir 0-0 Ash; Bay 3-2 Cove; Dale 0-2 Bay; \
Elm 1-2 Bay; Bay 2-2 Fir; Cove 1-3 Dale; Cove 2-0 Elm; Fir 1-0 Cove; Elm 0-0 Dale; Dale 2-2 Fir; Elm 3-2 Fir";

#[derive(Default, Clone)]
struct Row {
    name: String,
    p: i32,
    w: i32,
    d: i32,
    l: i32,
    gf: i32,
    ga: i32,
    pts: i32,
    h2h: i32,
}

fn points(a: i32, b: i32) -> i32 {
    if a > b { 3 } else if a == b { 1 } else { 0 }
}

fn main() {
    let mut matches: Vec<(String, i32, i32, String)> = Vec::new();
    for m in RESULTS.split("; ") {
        let w: Vec<&str> = m.split(' ').collect();
        let (x, y) = w[1].split_once('-').unwrap();
        matches.push((w[0].to_string(), x.parse().unwrap(), y.parse().unwrap(), w[2].to_string()));
    }
    let mut rows: Vec<Row> = Vec::new();
    let idx = |rows: &mut Vec<Row>, n: &str| -> usize {
        if let Some(i) = rows.iter().position(|r| r.name == n) {
            i
        } else {
            rows.push(Row { name: n.to_string(), ..Default::default() });
            rows.len() - 1
        }
    };
    for (h, x, y, a) in &matches {
        let hi = idx(&mut rows, h);
        let ai = idx(&mut rows, a);
        for (i, f, g) in [(hi, *x, *y), (ai, *y, *x)] {
            let r = &mut rows[i];
            r.p += 1;
            r.gf += f;
            r.ga += g;
            r.pts += points(f, g);
            match f.cmp(&g) {
                std::cmp::Ordering::Greater => r.w += 1,
                std::cmp::Ordering::Equal => r.d += 1,
                std::cmp::Ordering::Less => r.l += 1,
            }
        }
    }
    let key = |r: &Row| (r.pts, r.gf - r.ga, r.gf);
    // Head-to-head points among the teams tied on criteria 1 to 3.
    let snapshot = rows.clone();
    for r in rows.iter_mut() {
        let group: Vec<&str> = snapshot.iter().filter(|o| key(o) == key(r)).map(|o| o.name.as_str()).collect();
        let mut h2h = 0;
        for (h, x, y, a) in &matches {
            if group.contains(&h.as_str()) && group.contains(&a.as_str()) {
                if *h == r.name {
                    h2h += points(*x, *y);
                } else if *a == r.name {
                    h2h += points(*y, *x);
                }
            }
        }
        r.h2h = h2h;
    }
    rows.sort_by(|a, b| {
        let ka = (a.pts, a.gf - a.ga, a.gf, a.h2h);
        let kb = (b.pts, b.gf - b.ga, b.gf, b.h2h);
        kb.cmp(&ka).then_with(|| a.name.cmp(&b.name))
    });
    println!("{:>3} {:<6} {:>2} {:>2} {:>2} {:>2} {:>3} {:>3} {:>3} {:>3}", "Pos", "Team", "P", "W", "D", "L", "GF", "GA", "GD", "Pts");
    for (i, r) in rows.iter().enumerate() {
        let gd = r.gf - r.ga;
        let gds = if gd > 0 { format!("+{}", gd) } else { gd.to_string() };
        println!(
            "{:>3} {:<6} {:>2} {:>2} {:>2} {:>2} {:>3} {:>3} {:>3} {:>3}",
            i + 1,
            r.name,
            r.p,
            r.w,
            r.d,
            r.l,
            r.gf,
            r.ga,
            gds,
            r.pts
        );
    }
}
