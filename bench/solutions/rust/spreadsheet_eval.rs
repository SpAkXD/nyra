use std::collections::HashMap;

const CELLS: [(&str, &str); 19] = [
    ("A1", "10"),
    ("B1", "=A1*2"),
    ("C1", "=B1+A2"),
    ("D1", "=SUM(A1:C1)"),
    ("A2", "3"),
    ("B2", "=A2-B1/4"),
    ("D2", "=MAX(A1:C2)*(C2+2)"),
    ("A3", "=B3+1"),
    ("B3", "=C3"),
    ("C3", "=A3*0"),
    ("D3", "=-C3"),
    ("A4", "=10/(A2-3)"),
    ("B4", "=A4+1"),
    ("C4", "=MAX(A1:B2)+D4"),
    ("D4", "=E1+1"),
    ("A5", "=SUM(A1:D2)-(A1+B1)*-2"),
    ("B5", "=B1/0+D4"),
    ("C5", "=SUM(C4:C5)"),
    ("D5", "=D4*0+B4"),
];

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(i64),
    Name(String),
    Op(char),
}

fn lex(s: &str) -> Vec<Tok> {
    let b: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_digit() {
            let mut n = 0i64;
            while i < b.len() && b[i].is_ascii_digit() {
                n = n * 10 + b[i].to_digit(10).unwrap() as i64;
                i += 1;
            }
            out.push(Tok::Num(n));
        } else if c.is_ascii_alphabetic() {
            let mut t = String::new();
            while i < b.len() && b[i].is_ascii_alphanumeric() {
                t.push(b[i]);
                i += 1;
            }
            out.push(Tok::Name(t));
        } else if c == ' ' {
            i += 1;
        } else {
            out.push(Tok::Op(c));
            i += 1;
        }
    }
    out
}

// (column, row) 0-based, or None when outside A1:D5.
fn cell_pos(name: &str) -> Option<(usize, usize)> {
    let col = name.chars().next()?;
    let row: usize = name[1..].parse().ok()?;
    if !('A'..='D').contains(&col) || !(1..=5).contains(&row) {
        return None;
    }
    Some(((col as u8 - b'A') as usize, row - 1))
}

fn cell_name(c: usize, r: usize) -> String {
    format!("{}{}", (b'A' + c as u8) as char, r + 1)
}

fn rect(a: &str, b: &str) -> Option<Vec<String>> {
    let (c1, r1) = cell_pos(a)?;
    let (c2, r2) = cell_pos(b)?;
    let mut v = Vec::new();
    for r in r1..=r2 {
        for c in c1..=c2 {
            v.push(cell_name(c, r));
        }
    }
    Some(v)
}

// Cells a formula refers to (inside the sheet).
fn refs(formula: &str) -> Vec<String> {
    let toks = lex(formula);
    let mut out = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        if let Tok::Name(n) = &toks[i] {
            if n == "SUM" || n == "MAX" {
                if let (Tok::Name(a), Tok::Name(b)) = (&toks[i + 2], &toks[i + 4]) {
                    if let Some(v) = rect(a, b) {
                        out.extend(v);
                    }
                }
                i += 6;
                continue;
            }
            if cell_pos(n).is_some() {
                out.push(n.clone());
            }
        }
        i += 1;
    }
    out
}

type Val = Result<i64, String>;

struct Sheet {
    src: HashMap<String, String>,
    memo: HashMap<String, Val>,
}

impl Sheet {
    fn value(&mut self, name: &str) -> Val {
        if let Some(v) = self.memo.get(name) {
            return v.clone();
        }
        let v = match self.src.get(name).cloned() {
            None => Ok(0),
            Some(s) => match s.strip_prefix('=') {
                None => Ok(s.parse().unwrap()),
                Some(f) => {
                    let toks = lex(f);
                    let mut p = 0;
                    self.expr(&toks, &mut p)
                }
            },
        };
        self.memo.insert(name.to_string(), v.clone());
        v
    }

    fn expr(&mut self, t: &[Tok], p: &mut usize) -> Val {
        let mut acc = self.term(t, p)?;
        while *p < t.len() && (t[*p] == Tok::Op('+') || t[*p] == Tok::Op('-')) {
            let op = t[*p].clone();
            *p += 1;
            let r = self.term(t, p)?;
            acc = if op == Tok::Op('+') { acc + r } else { acc - r };
        }
        Ok(acc)
    }

    fn term(&mut self, t: &[Tok], p: &mut usize) -> Val {
        let mut acc = self.unary(t, p)?;
        while *p < t.len() && (t[*p] == Tok::Op('*') || t[*p] == Tok::Op('/')) {
            let op = t[*p].clone();
            *p += 1;
            let r = self.unary(t, p)?;
            if op == Tok::Op('*') {
                acc *= r;
            } else {
                if r == 0 {
                    return Err("#DIV0".to_string());
                }
                acc /= r;
            }
        }
        Ok(acc)
    }

    fn unary(&mut self, t: &[Tok], p: &mut usize) -> Val {
        if t[*p] == Tok::Op('-') {
            *p += 1;
            return Ok(-self.unary(t, p)?);
        }
        self.primary(t, p)
    }

    fn primary(&mut self, t: &[Tok], p: &mut usize) -> Val {
        match t[*p].clone() {
            Tok::Num(n) => {
                *p += 1;
                Ok(n)
            }
            Tok::Op('(') => {
                *p += 1;
                let v = self.expr(t, p)?;
                *p += 1; // ')'
                Ok(v)
            }
            Tok::Name(n) if n == "SUM" || n == "MAX" => {
                let (a, b) = match (&t[*p + 2], &t[*p + 4]) {
                    (Tok::Name(a), Tok::Name(b)) => (a.clone(), b.clone()),
                    _ => panic!("bad range"),
                };
                *p += 6;
                let cells = rect(&a, &b).ok_or_else(|| "#REF".to_string())?;
                let mut vals = Vec::new();
                for c in cells {
                    vals.push(self.value(&c)?);
                }
                Ok(if n == "SUM" { vals.iter().sum() } else { *vals.iter().max().unwrap() })
            }
            Tok::Name(n) => {
                *p += 1;
                if cell_pos(&n).is_none() {
                    return Err("#REF".to_string());
                }
                self.value(&n)
            }
            t => panic!("unexpected {:?}", t),
        }
    }
}

fn main() {
    let mut src = HashMap::new();
    let mut graph: HashMap<String, Vec<String>> = HashMap::new();
    for (n, s) in CELLS.iter() {
        src.insert(n.to_string(), s.to_string());
        let r = if let Some(f) = s.strip_prefix('=') { refs(f) } else { Vec::new() };
        graph.insert(n.to_string(), r);
    }
    // A cell is on a cycle if it can reach itself.
    let reach = |start: &str| -> Vec<String> {
        let mut seen: Vec<String> = Vec::new();
        let mut stack: Vec<String> = graph.get(start).cloned().unwrap_or_default();
        while let Some(c) = stack.pop() {
            if seen.contains(&c) {
                continue;
            }
            seen.push(c.clone());
            stack.extend(graph.get(&c).cloned().unwrap_or_default());
        }
        seen
    };
    let on_cycle: Vec<String> = CELLS.iter().map(|c| c.0.to_string()).filter(|c| reach(c).contains(c)).collect();
    let mut memo: HashMap<String, Val> = HashMap::new();
    for (n, _) in CELLS.iter() {
        if on_cycle.contains(&n.to_string()) || reach(n).iter().any(|c| on_cycle.contains(c)) {
            memo.insert(n.to_string(), Err("#CYCLE".to_string()));
        }
    }
    let mut sheet = Sheet { src, memo };
    for (n, _) in CELLS.iter() {
        match sheet.value(n) {
            Ok(v) => println!("{} = {}", n, v),
            Err(e) => println!("{} = {}", n, e),
        }
    }
}
