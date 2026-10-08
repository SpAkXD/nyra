const EXPRESSIONS: [&str; 18] = [
    "1 + 2 * 3 - 4 / 2",
    "10 - 4 - 3",
    "2 ^ 3 ^ 2",
    "-2 ^ 2",
    "(-2) ^ 2",
    "2 * (3 + 4) ^ 2",
    "100 / 7 % 4",
    "- - 3",
    "8/3*3+8%3",
    "((15 % 4) ^ 2 - -3) * 2",
    "0 * (5 / (2 - 2))",
    "0^0 + 2^10 - 1000",
    "-(3 - 10) * -2 ^ 3",
    "7 - (2 - (3 - (4 - 5)))",
    "2 ^ (1 + 1) ^ 3",
    "17 % 5 ^ 2 / 3",
    "(1 + 2) * (3 % (4 - 4)) + 1",
    "-3 ^ 2 * -(1 + 1) ^ 2",
];

// None means a division by zero happened somewhere.
type Val = Option<i128>;

struct Parser {
    toks: Vec<char>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.toks.get(self.pos).copied()
    }

    fn sum(&mut self) -> Val {
        let mut acc = self.product();
        while let Some(op) = self.peek() {
            if op != '+' && op != '-' {
                break;
            }
            self.pos += 1;
            let rhs = self.product();
            acc = match (acc, rhs) {
                (Some(a), Some(b)) => Some(if op == '+' { a + b } else { a - b }),
                _ => None,
            };
        }
        acc
    }

    fn product(&mut self) -> Val {
        let mut acc = self.unary();
        while let Some(op) = self.peek() {
            if op != '*' && op != '/' && op != '%' {
                break;
            }
            self.pos += 1;
            let rhs = self.unary();
            acc = match (acc, rhs) {
                (Some(a), Some(b)) => match op {
                    '*' => Some(a * b),
                    _ if b == 0 => None,
                    '/' => Some(a / b),
                    _ => Some(a % b),
                },
                _ => None,
            };
        }
        acc
    }

    fn unary(&mut self) -> Val {
        if self.peek() == Some('-') {
            self.pos += 1;
            return self.unary().map(|v| -v);
        }
        self.power()
    }

    fn power(&mut self) -> Val {
        let base = self.atom();
        if self.peek() == Some('^') {
            self.pos += 1;
            let exp = self.power();
            return match (base, exp) {
                (Some(a), Some(b)) => {
                    let mut r: i128 = 1;
                    for _ in 0..b {
                        r *= a;
                    }
                    Some(r)
                }
                _ => None,
            };
        }
        base
    }

    fn atom(&mut self) -> Val {
        if self.peek() == Some('(') {
            self.pos += 1;
            let v = self.sum();
            self.pos += 1; // ')'
            return v;
        }
        let mut n: i128 = 0;
        while let Some(c) = self.peek() {
            if let Some(d) = c.to_digit(10) {
                n = n * 10 + d as i128;
                self.pos += 1;
            } else {
                break;
            }
        }
        Some(n)
    }
}

fn main() {
    for e in EXPRESSIONS.iter() {
        let mut p = Parser { toks: e.chars().filter(|c| *c != ' ').collect(), pos: 0 };
        // digits separated by spaces never occur in the list, so dropping spaces is safe
        match p.sum() {
            Some(v) => println!("{} = {}", e, v),
            None => println!("{} = division by zero", e),
        }
    }
}
