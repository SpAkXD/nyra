programs = [
    ["push 9875", "call digits", "print", "push 406", "call digits", "print", "halt",
     "digits:", "push 0", "loop:", "over", "jz done", "over", "push 10", "mod", "add", "swap", "push 10", "div",
     "swap", "jmp loop", "done:", "swap", "pop", "ret"],
    ["push 5", "push 3", "sub", "dup", "print", "push 7", "swap", "sub", "print", "push -4", "ret", "print"],
    ["push 3", "top:", "dup", "print", "push 2", "mul", "jmp top"],
    ["push 6", "push 4", "call f", "print", "f:", "over", "over", "mod", "jz g", "push 0", "div", "g:", "ret"],
    ["push 1", "push 2", "swap", "over", "add", "add", "mul", "push 8"],
]
STEP_LIMIT = 100
NEEDS = {"pop": 1, "dup": 1, "swap": 2, "over": 2, "add": 2, "sub": 2, "mul": 2, "div": 2, "mod": 2, "print": 1,
         "jz": 1}


def run(lines):
    labels = {line[:-1]: i for i, line in enumerate(lines) if line.endswith(":")}
    stack, calls = [], []
    pc, steps = 0, 0
    while pc < len(lines):
        line = lines[pc]
        if line.endswith(":"):
            pc += 1
            continue
        if steps == STEP_LIMIT:
            print("error: step limit")
            break
        steps += 1
        parts = line.split(" ")
        op = parts[0]
        here = pc + 1
        pc += 1
        if len(stack) < NEEDS.get(op, 0):
            print(f"error at line {here}: stack underflow")
            break
        if op == "push":
            stack.append(int(parts[1]))
        elif op == "pop":
            stack.pop()
        elif op == "dup":
            stack.append(stack[-1])
        elif op == "swap":
            stack[-1], stack[-2] = stack[-2], stack[-1]
        elif op == "over":
            stack.append(stack[-2])
        elif op in ("add", "sub", "mul", "div", "mod"):
            if op in ("div", "mod") and stack[-1] == 0:
                print(f"error at line {here}: division by zero")
                break
            b = stack.pop()
            a = stack.pop()
            stack.append({"add": a + b, "sub": a - b, "mul": a * b,
                          "div": a // b if op == "div" else 0, "mod": a % b if op == "mod" else 0}[op])
        elif op == "print":
            print(stack.pop())
        elif op == "jmp":
            pc = labels[parts[1]]
        elif op == "jz":
            if stack.pop() == 0:
                pc = labels[parts[1]]
        elif op == "call":
            calls.append(pc)
            pc = labels[parts[1]]
        elif op == "ret":
            if not calls:
                print(f"error at line {here}: return without call")
                break
            pc = calls.pop()
        elif op == "halt":
            break
    print("stack: " + (" ".join(str(v) for v in stack) or "empty"))


for k, lines in enumerate(programs, 1):
    print(f"program {k}")
    run(lines)
