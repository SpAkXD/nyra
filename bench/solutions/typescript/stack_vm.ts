const programs: string[][] = [
  ["push 9875", "call digits", "print", "push 406", "call digits", "print", "halt", "digits:", "push 0", "loop:", "over", "jz done", "over", "push 10", "mod", "add", "swap", "push 10", "div", "swap", "jmp loop", "done:", "swap", "pop", "ret"],
  ["push 5", "push 3", "sub", "dup", "print", "push 7", "swap", "sub", "print", "push -4", "ret", "print"],
  ["push 3", "top:", "dup", "print", "push 2", "mul", "jmp top"],
  ["push 6", "push 4", "call f", "print", "f:", "over", "over", "mod", "jz g", "push 0", "div", "g:", "ret"],
  ["push 1", "push 2", "swap", "over", "add", "add", "mul", "push 8"],
];

const need: Record<string, number> = { pop: 1, dup: 1, print: 1, jz: 1, swap: 2, over: 2, add: 2, sub: 2, mul: 2, div: 2, mod: 2 };

function run(prog: string[]): void {
  const labels = new Map<string, number>();
  prog.forEach((l, i) => {
    if (l.endsWith(":")) labels.set(l.slice(0, -1), i);
  });
  const stack: number[] = [];
  const rets: number[] = [];
  let pc = 0;
  let steps = 0;
  while (pc < prog.length) {
    const line = prog[pc];
    if (line.endsWith(":")) {
      pc++;
      continue;
    }
    if (steps === 100) {
      console.log("error: step limit");
      break;
    }
    steps++;
    const [op, arg] = line.split(" ");
    const n = pc + 1;
    if ((need[op] ?? 0) > stack.length) {
      console.log(`error at line ${n}: stack underflow`);
      break;
    }
    if ((op === "div" || op === "mod") && stack[stack.length - 1] === 0) {
      console.log(`error at line ${n}: division by zero`);
      break;
    }
    if (op === "ret" && rets.length === 0) {
      console.log(`error at line ${n}: return without call`);
      break;
    }
    pc++;
    if (op === "push") stack.push(Number(arg));
    else if (op === "pop") stack.pop();
    else if (op === "dup") stack.push(stack[stack.length - 1]);
    else if (op === "swap") {
      const b = stack.pop()!;
      const a = stack.pop()!;
      stack.push(b, a);
    } else if (op === "over") stack.push(stack[stack.length - 2]);
    else if (["add", "sub", "mul", "div", "mod"].includes(op)) {
      const b = stack.pop()!;
      const a = stack.pop()!;
      stack.push(op === "add" ? a + b : op === "sub" ? a - b : op === "mul" ? a * b : op === "div" ? Math.trunc(a / b) : a % b);
    } else if (op === "print") console.log(String(stack.pop()));
    else if (op === "jmp") pc = labels.get(arg)! + 1;
    else if (op === "jz") {
      if (stack.pop() === 0) pc = labels.get(arg)! + 1;
    } else if (op === "call") {
      rets.push(pc);
      pc = labels.get(arg)! + 1;
    } else if (op === "ret") pc = rets.pop()!;
    else if (op === "halt") break;
  }
  console.log(stack.length ? `stack: ${stack.join(" ")}` : "stack: empty");
}

programs.forEach((p, i) => {
  console.log(`program ${i + 1}`);
  run(p);
});
