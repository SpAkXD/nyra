import re

ALLOWED = set("abcdefghijklmnopqrstuvwxyz.~")
PLAIN = set("abcdefghijklmnopqrstuvwxyz.")

def encode(text):
    out = []
    i = 0
    while i < len(text):
        c = text[i]
        j = i
        while j < len(text) and text[j] == c:
            j += 1
        n = j - i
        if c == '~':
            out.append("~%d~" % n)
        elif n >= 4:
            out.append("~%d%s" % (n, c))
        else:
            out.append(c * n)
        i = j
    return "".join(out)

def decode(code):
    out = []
    i = 0
    L = len(code)
    while i < L:
        ch = code[i]
        if ch == '~':
            i += 1
            if i >= L or not code[i].isdigit() or code[i] == '0':
                return None
            j = i
            while j < L and code[j] in "0123456789":
                j += 1
            n = int(code[i:j])
            if j >= L:
                return None
            c = code[j]
            if c not in ALLOWED:
                return None
            out.append(c * n)
            i = j + 1
        else:
            if ch not in PLAIN:
                return None
            out.append(ch)
            i += 1
    return "".join(out)

texts = ["aaaabbbcccccd", "~~x", ".......", "abc", "z" * 14 + "~", "a~~~~b"]
for t in texts:
    print("encode %s -> %s" % (t, encode(t)))

codes = ["~4a~3~b", "~04a", "~2ab", "a~b", "~12z", "~5", "ab~3~~",
         "x~1~y", "q7", "~10.~4~", "~1~~1~", "a~3B"]
for c in codes:
    t = decode(c)
    if t is None:
        print("decode %s -> invalid" % c)
    elif encode(t) == c:
        print("decode %s -> %s" % (c, t))
    else:
        print("decode %s -> %s (non-canonical)" % (c, t))
