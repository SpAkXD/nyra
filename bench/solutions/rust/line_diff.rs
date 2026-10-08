fn diff(a: &[&str], b: &[&str]) {
    let n = a.len();
    let m = b.len();
    let mut l = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            l[i][j] = if a[i] == b[j] { l[i + 1][j + 1] + 1 } else { l[i + 1][j].max(l[i][j + 1]) };
        }
    }
    // script entries: (kind, line)
    let mut script: Vec<(char, &str)> = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            script.push(('=', a[i]));
            i += 1;
            j += 1;
        } else if i < n && (j == m || l[i + 1][j] >= l[i][j + 1]) {
            script.push(('-', a[i]));
            i += 1;
        } else {
            script.push(('+', b[j]));
            j += 1;
        }
    }
    let (mut ins, mut del, mut same) = (0, 0, 0);
    let mut k = 0;
    while k < script.len() {
        let (kind, line) = script[k];
        if kind == '=' {
            let mut e = k;
            while e < script.len() && script[e].0 == '=' {
                e += 1;
            }
            let run = e - k;
            same += run;
            if run >= 3 {
                println!("= ({} unchanged)", run);
            } else {
                for s in &script[k..e] {
                    println!("= {}", s.1);
                }
            }
            k = e;
            continue;
        }
        if kind == '-' {
            del += 1;
        } else {
            ins += 1;
        }
        println!("{} {}", kind, line);
        k += 1;
    }
    println!("summary: +{} -{} ={}", ins, del, same);
}

fn main() {
    println!("diff 1");
    diff(
        &["alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa"],
        &["alpha", "gamma", "delta", "beta", "epsilon", "zeta", "eta", "lambda", "theta", "kappa", "mu"],
    );
    println!("diff 2");
    diff(
        &["x = 1", "y = 2", "print(x)", "print(y)", "z = x + y", "print(z)"],
        &["y = 2", "x = 1", "print(x)", "z = x + y", "print(z)", "print(y)"],
    );
}
