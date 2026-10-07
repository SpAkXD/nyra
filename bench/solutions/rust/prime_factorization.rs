fn factors(n: u64) -> Vec<(u64, u32)> {
    let mut result: Vec<(u64, u32)> = Vec::new();
    let mut rest = n;
    let mut p = 2;
    while rest > 1 {
        if p * p > rest {
            p = rest;
        }
        while rest % p == 0 {
            rest /= p;
            match result.last_mut() {
                Some((q, e)) if *q == p => *e += 1,
                _ => result.push((p, 1)),
            }
        }
        p += 1;
    }
    result
}

fn main() {
    for n in [360, 97, 1001, 65536, 999999] {
        let parts: Vec<String> = factors(n)
            .iter()
            .map(|&(p, e)| if e > 1 { format!("{}^{}", p, e) } else { p.to_string() })
            .collect();
        println!("{} = {}", n, parts.join(" * "));
    }
}
