snippets = [
    ["0|fn main() {", "4|let xs = [1, 2,", "4|3]", "4|if x {", "8|call(a, [b,", "8|c])", "4|}", "0|}"],
    ["0|f(a, {", "4|b: [1, 2)", "0|})"],
    ["0|g {", "2|h()", "0|}"],
    ["0|a {", "4|b {", "8|c", "4|}", "4|}"],
    ["0|x", "0|)"],
    ["0|k(", "4|m[", "8|n", "4|]"],
    ["2|a"],
    ["0|f() {", "0|}", "0|g [", "4|1, (2", "4|)]"],
    ["0|if a {", "4|b", "0|} else {", "4|c", "0|}"],
    ["0|p {", "3|q)"],
    ["0|r([", "4|s", "0|])", "0|t"],
]
OPEN = "([{"
CLOSE = ")]}"


def check(lines):
    stack = []  # (bracket, line number, indentation of that line)
    prev_indent, prev_text = None, None
    for number, line in enumerate(lines, 1):
        indent_text, text = line.split("|", 1)
        indent = int(indent_text)
        closer_opener = None
        for pos, ch in enumerate(text):
            if ch in OPEN:
                stack.append((ch, number, indent))
            elif ch in CLOSE:
                if not stack:
                    return f"line {number}: unexpected {ch}"
                want = CLOSE[OPEN.index(stack[-1][0])]
                if ch != want:
                    return f"line {number}: expected {want} but found {ch}"
                opener = stack.pop()
                if pos == 0:
                    closer_opener = opener
        if number == 1:
            expected = 0
        elif closer_opener is not None:
            expected = closer_opener[2]
        elif prev_text[-1] in OPEN:
            expected = prev_indent + 4
        else:
            expected = prev_indent
        if indent != expected:
            return f"line {number}: indent {indent}, expected {expected}"
        prev_indent, prev_text = indent, text
    if stack:
        return f"line {stack[-1][1]}: unclosed {stack[-1][0]}"
    return "ok"


for k, lines in enumerate(snippets, 1):
    print(f"snippet {k}: {check(lines)}")
