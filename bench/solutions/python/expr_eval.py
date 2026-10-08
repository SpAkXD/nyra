expressions = [
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
]


class DivisionByZero(Exception):
    pass


def tokenize(text):
    tokens = []
    i = 0
    while i < len(text):
        c = text[i]
        if c == " ":
            i += 1
        elif c.isdigit():
            j = i
            while j < len(text) and text[j].isdigit():
                j += 1
            tokens.append(int(text[i:j]))
            i = j
        else:
            tokens.append(c)
            i += 1
    return tokens


class Parser:
    def __init__(self, tokens):
        self.tokens = tokens
        self.pos = 0

    def peek(self):
        return self.tokens[self.pos] if self.pos < len(self.tokens) else None

    def take(self):
        tok = self.tokens[self.pos]
        self.pos += 1
        return tok

    def sum(self):
        value = self.product()
        while self.peek() in ("+", "-"):
            op = self.take()
            right = self.product()
            value = value + right if op == "+" else value - right
        return value

    def product(self):
        value = self.unary()
        while self.peek() in ("*", "/", "%"):
            op = self.take()
            right = self.unary()
            if op == "*":
                value *= right
            elif right == 0:
                raise DivisionByZero()
            elif op == "/":
                value //= right
            else:
                value %= right
        return value

    def unary(self):
        if self.peek() == "-":
            self.take()
            return -self.unary()
        return self.power()

    def power(self):
        base = self.atom()
        if self.peek() == "^":
            self.take()
            exponent = self.power()
            result = 1
            for _ in range(exponent):
                result *= base
            return result
        return base

    def atom(self):
        tok = self.take()
        if tok == "(":
            value = self.sum()
            self.take()  # ")"
            return value
        return tok


for text in expressions:
    try:
        result = Parser(tokenize(text)).sum()
    except DivisionByZero:
        result = "division by zero"
    print(f"{text} = {result}")
