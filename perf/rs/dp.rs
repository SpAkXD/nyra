// knapsack (one array) and LCS (a 2D table) (reference for dp.nyra)
fn knapsack(weights: &[i64], values: &[i64], cap: i64) -> i64 {
    let mut best = vec![0i64; cap as usize + 1];
    for k in 0..weights.len() {
        let w = weights[k];
        let v = values[k];
        let mut c = cap;
        while c >= w {
            let take = best[(c - w) as usize] + v;
            if take > best[c as usize] {
                best[c as usize] = take;
            }
            c -= 1;
        }
    }
    best[cap as usize]
}

fn lcs(a: &[i64], b: &[i64]) -> i64 {
    let (n, m) = (a.len(), b.len());
    let mut dp = vec![vec![0i64; m + 1]; n + 1];
    for i in 1..=n {
        for j in 1..=m {
            dp[i][j] = if a[i - 1] == b[j - 1] { dp[i - 1][j - 1] + 1 } else { dp[i - 1][j].max(dp[i][j - 1]) };
        }
    }
    dp[n][m]
}

fn main() {
    let mut seed: i64 = 42;
    let mut next = || {
        seed = (seed * 1103515245 + 12345) % 2147483648;
        seed
    };
    let (mut weights, mut values) = (Vec::new(), Vec::new());
    for _ in 0..1000 {
        weights.push((next() / 65536) % 1000 + 1);
        values.push((next() / 65536) % 1000 + 1);
    }
    println!("{}", knapsack(&weights, &values, 50000));
    let (mut a, mut b) = (Vec::new(), Vec::new());
    for _ in 0..2500 {
        a.push((next() / 65536) % 4);
        b.push((next() / 65536) % 4);
    }
    println!("{}", lcs(&a, &b));
}
