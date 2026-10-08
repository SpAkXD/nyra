type M = Vec<Vec<i64>>;

fn parse(s: &str) -> M {
    s.split(';')
        .map(|r| r.split_whitespace().map(|x| x.parse().unwrap()).collect())
        .collect()
}

fn mul(a: &M, b: &M) -> M {
    let n = a.len();
    let k = b.len();
    let m = b[0].len();
    let mut r = vec![vec![0i64; m]; n];
    for i in 0..n {
        for j in 0..m {
            for t in 0..k {
                r[i][j] += a[i][t] * b[t][j];
            }
        }
    }
    r
}

fn transpose(a: &M) -> M {
    (0..a[0].len()).map(|j| a.iter().map(|row| row[j]).collect()).collect()
}

fn show(title: &str, a: &M) {
    println!("{}", title);
    let cols = a[0].len();
    let widths: Vec<usize> = (0..cols)
        .map(|j| a.iter().map(|row| row[j].to_string().len()).max().unwrap())
        .collect();
    for row in a {
        let cells: Vec<String> = row
            .iter()
            .enumerate()
            .map(|(j, v)| format!("{:>w$}", v, w = widths[j]))
            .collect();
        println!("{}", cells.join("  "));
    }
}

fn det(a: &M) -> i64 {
    let n = a.len();
    if n == 1 {
        return a[0][0];
    }
    let mut total = 0;
    for c in 0..n {
        let minor: M = a[1..]
            .iter()
            .map(|row| row.iter().enumerate().filter(|(j, _)| *j != c).map(|(_, v)| *v).collect())
            .collect();
        let sign = if c % 2 == 0 { 1 } else { -1 };
        total += sign * a[0][c] * det(&minor);
    }
    total
}

fn main() {
    let a = parse("2 -1 0; 1 3 -2; 0 4 1");
    let b = parse("1 2; -3 0; 5 -1");
    let c = parse("2 -1; 1 3");
    let d = parse("3 1 2 -1; 1 2 0 -2; 4 -1 6 -3; 5 0 2 1");

    let ab = mul(&a, &b);
    show("A*B", &ab);
    show("(A*B)^T", &transpose(&ab));
    let mut c6 = c.clone();
    for _ in 0..5 {
        c6 = mul(&c6, &c);
    }
    show("C^6", &c6);
    let aa = mul(&a, &a);
    let mut r = aa.clone();
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] = aa[i][j] - 3 * a[i][j] + if i == j { 2 } else { 0 };
        }
    }
    show("A*A-3A+2I", &r);
    println!("det(D) = {}", det(&d));
}
