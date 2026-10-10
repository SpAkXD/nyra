import re

def step(s, pat=re.compile(r'(\d)\1*')):
    return ''.join([str(len(m.group())) + m.group(1) for m in pat.finditer(s)])

def main():
    s = "2333111"
    out = []
    for i in range(1, 41):
        s = step(s)
        if i % 5 == 0:
            out.append(f"{i} {len(s)}")
    out.append(f"{s.count('1')} {s.count('2')} {s.count('3')}")
    print("\n".join(out))

main()
