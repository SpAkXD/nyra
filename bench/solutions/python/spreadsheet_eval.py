cells = {
    "A1": "10", "B1": "=A1*2", "C1": "=B1+A2", "D1": "=SUM(A1:C1)",
    "A2": "3", "B2": "=A2-B1/4", "D2": "=MAX(A1:C2)*(C2+2)",
    "A3": "=B3+1", "B3": "=C3", "C3": "=A3*0", "D3": "=-C3",
    "A4": "=10/(A2-3)", "B4": "=A4+1", "C4": "=MAX(A1:B2)+D4", "D4": "=E1+1",
    "A5": "=SUM(A1:D2)-(A1+B1)*-2", "B5": "=B1/0+D4", "C5": "=SUM(C4:C5)", "D5": "=D4*0+B4",
}
COLS, ROWS = "ABCD", 5
ORDER = [c + str(r) for r in range(1, ROWS + 1) for c in COLS]


class Err(Exception):
    def __init__(self, code):
        self.code = code


def tokenize(text):
    tokens, i = [], 0
    while i < len(text):
        ch = text[i]
        if ch.isdigit():
            j = i
            while j < len(text) and text[j].isdigit():
                j += 1
            tokens.append(int(text[i:j]))
            i = j
        elif ch.isalpha():
            j = i
            while j < len(text) and text[j].isalnum():
                j += 1
            tokens.append(text[i:j])
            i = j
        else:
            tokens.append(ch)
            i += 1
    return tokens


def in_grid(name):
    return name[0] in COLS and 1 <= int(name[1:]) <= ROWS


def range_cells(a, b):
    cols = COLS[COLS.index(a[0]):COLS.index(b[0]) + 1]
    return [c + str(r) for r in range(int(a[1:]), int(b[1:]) + 1) for c in cols]


def references(name):
    """Cells inside the grid that the formula in `name` refers to."""
    text = cells.get(name, "")
    if not text.startswith("="):
        return []
    tokens = tokenize(text[1:])
    out = []
    for k, tok in enumerate(tokens):
        if isinstance(tok, str) and tok[0].isalpha() and tok not in ("SUM", "MAX"):
            if k + 1 < len(tokens) and tokens[k + 1] == ":":
                end = tokens[k + 2]
                if in_grid(tok) and in_grid(end):
                    out += range_cells(tok, end)
            elif not (k > 0 and tokens[k - 1] == ":") and in_grid(tok):
                out.append(tok)
    return out


def reaches(start, target, seen):
    for nxt in references(start):
        if nxt == target:
            return True
        if nxt not in seen:
            seen.add(nxt)
            if reaches(nxt, target, seen):
                return True
    return False


on_cycle = {name for name in ORDER if reaches(name, name, set())}


def cycle_state(name):
    if name in on_cycle:
        return True
    seen = set()
    stack = [name]
    while stack:
        cur = stack.pop()
        for nxt in references(cur):
            if nxt in on_cycle:
                return True
            if nxt not in seen:
                seen.add(nxt)
                stack.append(nxt)
    return False


memo = {}


def value(name):
    """Value of a cell, raising Err for an error."""
    if name in memo:
        result = memo[name]
    else:
        text = cells.get(name)
        if text is None:
            result = 0
        elif not text.startswith("="):
            result = int(text)
        elif cycle_state(name):
            result = Err("#CYCLE")
        else:
            try:
                p = Parser(tokenize(text[1:]))
                result = p.expr()
            except Err as e:
                result = e
        memo[name] = result
    if isinstance(result, Err):
        raise result
    return result


class Parser:
    def __init__(self, tokens):
        self.tokens, self.pos = tokens, 0

    def peek(self):
        return self.tokens[self.pos] if self.pos < len(self.tokens) else None

    def take(self):
        self.pos += 1
        return self.tokens[self.pos - 1]

    def expr(self):
        v = self.term()
        while self.peek() in ("+", "-"):
            op = self.take()
            r = self.term()
            v = v + r if op == "+" else v - r
        return v

    def term(self):
        v = self.unary()
        while self.peek() in ("*", "/"):
            op = self.take()
            r = self.unary()
            if op == "*":
                v *= r
            elif r == 0:
                raise Err("#DIV0")
            else:
                v //= r
        return v

    def unary(self):
        if self.peek() == "-":
            self.take()
            return -self.unary()
        return self.atom()

    def atom(self):
        tok = self.take()
        if isinstance(tok, int):
            return tok
        if tok == "(":
            v = self.expr()
            self.take()
            return v
        if tok in ("SUM", "MAX"):
            self.take()  # (
            a = self.take()
            self.take()  # :
            b = self.take()
            self.take()  # )
            if not (in_grid(a) and in_grid(b)):
                raise Err("#REF")
            values = [value(c) for c in range_cells(a, b)]
            return sum(values) if tok == "SUM" else max(values)
        if not in_grid(tok):
            raise Err("#REF")
        return value(tok)


for name in ORDER:
    if name in cells:
        try:
            print(f"{name} = {value(name)}")
        except Err as e:
            print(f"{name} = {e.code}")
