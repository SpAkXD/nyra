fn sequence(seed: u32, n: usize) -> Vec<u32> {
    let mut out = Vec::with_capacity(n);
    let mut x = seed;
    for _ in 0..n {
        x = (x * 75 + 74) % 65537;
        out.push(x % 4);
    }
    out
}

fn main() {
    let n = 1000;
    let a = sequence(1, n);
    let b = sequence(2, n);
    let mut prev = vec![0u32; n + 1];
    for i in 1..=n {
        let mut cur = vec![0u32; n + 1];
        let ai = a[i - 1];
        for j in 1..=n {
            cur[j] = if ai == b[j - 1] {
                prev[j - 1] + 1
            } else if prev[j] >= cur[j - 1] {
                prev[j]
            } else {
                cur[j - 1]
            };
        }
        prev = cur;
        if i % 250 == 0 {
            println!("{}", prev[i]);
        }
    }
}
