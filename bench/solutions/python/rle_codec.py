texts = ["aaaabbbcccccd", "~~x", ".......", "abc", "zzzzzzzzzzzzzz~", "a~~~~b"]
codes = ["~4a~3~b", "~04a", "~2ab", "a~b", "~12z", "~5", "ab~3~~", "x~1~y", "q7", "~10.~4~", "~1~~1~", "a~3B"]


def plain(c):
    return c.islower() or c == "."


def encode(text):
    out = []
    i = 0
    while i < len(text):
        j = i
        while j < len(text) and text[j] == text[i]:
            j += 1
        c, n = text[i], j - i
        if c == "~":
            out.append(f"~{n}~")
        elif n >= 4:
            out.append(f"~{n}{c}")
        else:
            out.append(c * n)
        i = j
    return "".join(out)


def decode(code):
    """The decoded text, or None if the code is invalid."""
    out = []
    i = 0
    while i < len(code):
        c = code[i]
        if c != "~":
            if not plain(c):
                return None
            out.append(c)
            i += 1
            continue
        j = i + 1
        while j < len(code) and code[j].isdigit():
            j += 1
        digits = code[i + 1:j]
        if digits == "" or digits[0] == "0" or j >= len(code):
            return None
        c = code[j]
        if not (plain(c) or c == "~"):
            return None
        out.append(c * int(digits))
        i = j + 1
    return "".join(out)


for text in texts:
    print(f"encode {text} -> {encode(text)}")
for code in codes:
    text = decode(code)
    if text is None:
        print(f"decode {code} -> invalid")
    elif encode(text) == code:
        print(f"decode {code} -> {text}")
    else:
        print(f"decode {code} -> {text} (non-canonical)")
