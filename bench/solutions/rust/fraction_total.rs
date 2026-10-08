const OPS: &str = "add 1/2; sub 3/4; mul -2/3; div 5/6; add 7/3; div 0/5; mul 4/-6; add 10/4; sub -1/12; mul 0/7; \
add -9/4; div -3/2; sub 5/3; add 2/3; div 1/-4";

fn gcd(a: i64, b: i64) -> i64 {
    if b == 0 { a.abs() } else { gcd(b, a % b) }
}

// Lowest terms with a positive denominator.
fn norm(n: i64, d: i64) -> (i64, i64) {
    let (n, d) = if d < 0 { (-n, -d) } else { (n, d) };
    let g = gcd(n, d).max(1);
    (n / g, d / g)
}

fn show((n, d): (i64, i64)) -> String {
    if d == 1 {
        return n.to_string();
    }
    let sign = if n < 0 { "-" } else { "" };
    let a = n.abs();
    if a < d {
        format!("{}{}/{}", sign, a, d)
    } else {
        format!("{}{} {}/{}", sign, a / d, a % d, d)
    }
}

fn main() {
    let mut total = (0i64, 1i64);
    let mut best: Option<((i64, i64), usize)> = None;
    for (i, op) in OPS.split("; ").enumerate() {
        let (word, frac) = op.split_once(' ').unwrap();
        let (a, b) = frac.split_once('/').unwrap();
        let (a, b): (i64, i64) = (a.parse().unwrap(), b.parse().unwrap());
        let (n, d) = total;
        let next = match word {
            "add" => norm(n * b + a * d, d * b),
            "sub" => norm(n * b - a * d, d * b),
            "mul" => norm(n * a, d * b),
            _ => {
                if a == 0 {
                    println!("{}: cannot divide by zero, total {}", op, show(total));
                    continue;
                }
                norm(n * b, d * a)
            }
        };
        total = next;
        println!("{}: total {}", op, show(total));
        let better = match best {
            None => true,
            Some(((bn, bd), _)) => total.0 * bd > bn * total.1,
        };
        if better {
            best = Some((total, i + 1));
        }
    }
    let (x, k) = best.unwrap();
    println!("largest total {} after operation {}", show(x), k);
}
