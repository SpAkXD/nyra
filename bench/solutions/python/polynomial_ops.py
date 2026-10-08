P_TEXT = "2x + 3 - x + x^2 - 4"
Q_TEXT = "-x^3 + 2x - 1 + x^3 + x^2 + x^10 - x^10"
R_TEXT = "5 - 3x^2"


def parse(text):
    """Coefficients by degree, as a dict."""
    poly = {}
    tokens = text.split(" ")
    sign = 1
    for tok in tokens:
        if tok == "+":
            sign = 1
            continue
        if tok == "-":
            sign = -1
            continue
        if tok.startswith("-"):
            sign, tok = -sign, tok[1:]
        if "x" in tok:
            coef_text, power_text = tok.split("x")
            coef = int(coef_text) if coef_text else 1
            degree = int(power_text[1:]) if power_text else 1
        else:
            coef, degree = int(tok), 0
        poly[degree] = poly.get(degree, 0) + sign * coef
        sign = 1
    return clean(poly)


def clean(poly):
    return {d: c for d, c in poly.items() if c != 0}


def add(p, q, factor=1):
    out = dict(p)
    for d, c in q.items():
        out[d] = out.get(d, 0) + factor * c
    return clean(out)


def mul(p, q):
    out = {}
    for d1, c1 in p.items():
        for d2, c2 in q.items():
            out[d1 + d2] = out.get(d1 + d2, 0) + c1 * c2
    return clean(out)


def derivative(p):
    return clean({d - 1: c * d for d, c in p.items() if d > 0})


def compose(p, q):
    out = {}
    for d, c in p.items():
        term = {0: c}
        for _ in range(d):
            term = mul(term, q)
        out = add(out, term)
    return out


def value(p, x):
    return sum(c * x ** d for d, c in p.items())


def show(p):
    if not p:
        return "0"
    parts = []
    for d in sorted(p, reverse=True):
        c = p[d]
        body = str(abs(c)) if d == 0 or abs(c) != 1 else ""
        if d == 1:
            body += "x"
        elif d > 1:
            body += f"x^{d}"
        if not parts:
            parts.append(("-" if c < 0 else "") + body)
        else:
            parts.append((" - " if c < 0 else " + ") + body)
    return "".join(parts)


P, Q, R = parse(P_TEXT), parse(Q_TEXT), parse(R_TEXT)
print(f"P = {show(P)}")
print(f"Q = {show(Q)}")
print(f"R = {show(R)}")
print(f"P + Q = {show(add(P, Q))}")
print(f"P - R = {show(add(P, R, -1))}")
print(f"P * Q = {show(mul(P, Q))}")
print(f"(P * Q)' = {show(derivative(mul(P, Q)))}")
print(f"Q(R) = {show(compose(Q, R))}")
print(f"R(P) - R = {show(add(compose(R, P), R, -1))}")
print(f"P - P = {show(add(P, P, -1))}")
print(f"P(-3) = {value(P, -3)}")
print(f"Q(R)(2) = {value(compose(Q, R), 2)}")
