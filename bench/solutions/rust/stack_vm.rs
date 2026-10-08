const PROGRAMS: [&[&str]; 5] = [
    &[
        "push 9875", "call digits", "print", "push 406", "call digits", "print", "halt", "digits:", "push 0", "loop:",
        "over", "jz done", "over", "push 10", "mod", "add", "swap", "push 10", "div", "swap", "jmp loop", "done:", "swap",
        "pop", "ret",
    ],
    &["push 5", "push 3", "sub", "dup", "print", "push 7", "swap", "sub", "print", "push -4", "ret", "print"],
    &["push 3", "top:", "dup", "print", "push 2", "mul", "jmp top"],
    &["push 6", "push 4", "call f", "print", "f:", "over", "over", "mod", "jz g", "push 0", "div", "g:", "ret"],
    &["push 1", "push 2", "swap", "over", "add", "add", "mul", "push 8"],
];

fn label(prog: &[&str], name: &str) -> usize {
    let want = format!("{}:", name);
    prog.iter().position(|l| *l == want).unwrap() + 1
}

fn run(prog: &[&str]) -> Vec<i64> {
    let mut stack: Vec<i64> = Vec::new();
    let mut rets: Vec<usize> = Vec::new();
    let mut pc = 0usize; // 0-based index of the next line
    let mut steps = 0;
    while pc < prog.len() {
        let line = prog[pc];
        if line.ends_with(':') {
            pc += 1;
            continue;
        }
        if steps == 100 {
            println!("error: step limit");
            break;
        }
        steps += 1;
        let n = pc + 1;
        let (op, arg) = match line.split_once(' ') {
            Some((o, a)) => (o, a),
            None => (line, ""),
        };
        let need = match op {
            "pop" | "dup" | "print" | "jz" => 1,
            "swap" | "over" | "add" | "sub" | "mul" | "div" | "mod" => 2,
            _ => 0,
        };
        if stack.len() < need {
            println!("error at line {}: stack underflow", n);
            break;
        }
        let len = stack.len();
        pc += 1;
        match op {
            "push" => stack.push(arg.parse().unwrap()),
            "pop" => {
                stack.pop();
            }
            "dup" => stack.push(stack[len - 1]),
            "swap" => stack.swap(len - 1, len - 2),
            "over" => stack.push(stack[len - 2]),
            "add" | "sub" | "mul" | "div" | "mod" => {
                let (a, b) = (stack[len - 2], stack[len - 1]);
                if (op == "div" || op == "mod") && b == 0 {
                    println!("error at line {}: division by zero", n);
                    break;
                }
                let r = match op {
                    "add" => a + b,
                    "sub" => a - b,
                    "mul" => a * b,
                    "div" => a / b,
                    _ => a % b,
                };
                stack.truncate(len - 2);
                stack.push(r);
            }
            "print" => println!("{}", stack.pop().unwrap()),
            "jmp" => pc = label(prog, arg),
            "jz" => {
                if stack.pop().unwrap() == 0 {
                    pc = label(prog, arg);
                }
            }
            "call" => {
                rets.push(pc);
                pc = label(prog, arg);
            }
            "ret" => match rets.pop() {
                Some(p) => pc = p,
                None => {
                    println!("error at line {}: return without call", n);
                    break;
                }
            },
            _ => break, // halt
        }
    }
    stack
}

fn main() {
    for (k, prog) in PROGRAMS.iter().enumerate() {
        println!("program {}", k + 1);
        let stack = run(prog);
        if stack.is_empty() {
            println!("stack: empty");
        } else {
            let s: Vec<String> = stack.iter().map(|v| v.to_string()).collect();
            println!("stack: {}", s.join(" "));
        }
    }
}
