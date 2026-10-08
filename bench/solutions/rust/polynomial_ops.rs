// A polynomial is its coefficients by degree (index = degree), trimmed of trailing zeros.
type Poly = Vec<i64>;

fn trim(mut p: Poly) -> Poly {
    while p.last() == Some(&0) {
        p.pop();
    }
    p
}

fn add_term(p: &mut Poly, deg: usize, c: i64) {
    if p.len() <= deg {
        p.resize(deg + 1, 0);
    }
    p[deg] += c;
}

fn parse_term(t: &str, sign: i64, p: &mut Poly) {
    let (coef_text, deg) = match t.find('x') {
        None => (t, 0usize),
        Some(i) => {
            let rest = &t[i + 1..];
            let deg = if rest.is_empty() { 1 } else { rest[1..].parse().unwrap() };
            (&t[..i], deg)
        }
    };
    let c: i64 = if coef_text.is_empty() { 1 } else { coef_text.parse().unwrap() };
    add_term(p, deg, sign * c);
}

fn parse(s: &str) -> Poly {
    let mut p = Vec::new();
    let tokens: Vec<&str> = s.split(' ').collect();
    let first = tokens[0];
    if let Some(rest) = first.strip_prefix('-') {
        parse_term(rest, -1, &mut p);
    } else {
        parse_term(first, 1, &mut p);
    }
    let mut i = 1;
    while i < tokens.len() {
        let sign = if tokens[i] == "-" { -1 } else { 1 };
        parse_term(tokens[i + 1], sign, &mut p);
        i += 2;
    }
    trim(p)
}

fn add(a: &Poly, b: &Poly, sb: i64) -> Poly {
    let mut r = vec![0; a.len().max(b.len())];
    for (i, c) in a.iter().enumerate() {
        r[i] += c;
    }
    for (i, c) in b.iter().enumerate() {
        r[i] += sb * c;
    }
    trim(r)
}

fn mul(a: &Poly, b: &Poly) -> Poly {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    let mut r = vec![0; a.len() + b.len() - 1];
    for (i, x) in a.iter().enumerate() {
        for (j, y) in b.iter().enumerate() {
            r[i + j] += x * y;
        }
    }
    trim(r)
}

fn deriv(a: &Poly) -> Poly {
    let mut r = Vec::new();
    for (i, c) in a.iter().enumerate().skip(1) {
        r.push(i as i64 * c);
    }
    trim(r)
}

fn compose(outer: &Poly, inner: &Poly) -> Poly {
    let mut r: Poly = Vec::new();
    for c in outer.iter().rev() {
        r = add(&mul(&r, inner), &vec![*c], 1);
    }
    r
}

fn eval(p: &Poly, x: i64) -> i64 {
    p.iter().rev().fold(0, |acc, c| acc * x + c)
}

fn show(p: &Poly) -> String {
    let mut out = String::new();
    for deg in (0..p.len()).rev() {
        let c = p[deg];
        if c == 0 {
            continue;
        }
        if out.is_empty() {
            if c < 0 {
                out.push('-');
            }
        } else {
            out.push_str(if c < 0 { " - " } else { " + " });
        }
        let a = c.abs();
        if a != 1 || deg == 0 {
            out.push_str(&a.to_string());
        }
        if deg == 1 {
            out.push('x');
        } else if deg >= 2 {
            out.push_str(&format!("x^{}", deg));
        }
    }
    if out.is_empty() {
        out.push('0');
    }
    out
}

fn main() {
    let p = parse("2x + 3 - x + x^2 - 4");
    let q = parse("-x^3 + 2x - 1 + x^3 + x^2 + x^10 - x^10");
    let r = parse("5 - 3x^2");
    let pq = mul(&p, &q);
    let qr = compose(&q, &r);
    println!("P = {}", show(&p));
    println!("Q = {}", show(&q));
    println!("R = {}", show(&r));
    println!("P + Q = {}", show(&add(&p, &q, 1)));
    println!("P - R = {}", show(&add(&p, &r, -1)));
    println!("P * Q = {}", show(&pq));
    println!("(P * Q)' = {}", show(&deriv(&pq)));
    println!("Q(R) = {}", show(&qr));
    println!("R(P) - R = {}", show(&add(&compose(&r, &p), &r, -1)));
    println!("P - P = {}", show(&add(&p, &p, -1)));
    println!("P(-3) = {}", eval(&p, -3));
    println!("Q(R)(2) = {}", eval(&qr, 2));
}
