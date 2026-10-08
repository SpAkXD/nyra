fn main() {
    let n = 120;
    let mut a = vec![vec![0i64; n]; n];
    let mut b = vec![vec![0i64; n]; n];
    for i in 0..n {
        for j in 0..n {
            a[i][j] = ((i * 31 + j * 17 + 7) % 100) as i64;
            b[i][j] = ((i * 13 + j * 29 + 3) % 100) as i64;
        }
    }
    let mut c = vec![vec![0i64; n]; n];
    for i in 0..n {
        for j in 0..n {
            let mut s = 0;
            for k in 0..n {
                s += a[i][k] * b[k][j];
            }
            c[i][j] = s;
        }
    }
    let mut trace = 0;
    let mut total = 0;
    for i in 0..n {
        trace += c[i][i];
        for j in 0..n {
            total += c[i][j];
        }
    }
    println!("{}", trace);
    println!("{}", total);
    println!("{}", c[0][n - 1]);
    println!("{}", c[n - 1][0]);
}
