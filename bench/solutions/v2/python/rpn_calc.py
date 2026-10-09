import re
import sys


def trunc_div(a, b):
    q = abs(a) // abs(b)
    return -q if (a < 0) != (b < 0) else q


for raw in sys.stdin.read().splitlines():
    tokens = [t for t in raw.split(" ") if t != ""]
    if not tokens:
        continue
    stack = []
    error = None
    for token in tokens:
        if re.fullmatch(r"-?[0-9]+", token):
            stack.append(int(token))
        elif token in ("+", "-", "*", "/"):
            if len(stack) < 2:
                error = "stack underflow"
                break
            b = stack.pop()
            a = stack.pop()
            if token == "+":
                stack.append(a + b)
            elif token == "-":
                stack.append(a - b)
            elif token == "*":
                stack.append(a * b)
            else:
                if b == 0:
                    error = "division by zero"
                    break
                stack.append(trunc_div(a, b))
        else:
            error = f"bad token {token}"
            break
    if error is None and len(stack) != 1:
        error = "leftover values"
    print(f"error: {error}" if error else stack[0])
