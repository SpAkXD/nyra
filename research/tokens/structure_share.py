"""Where do Python's tokens go?  Delete one class of characters from each program and see what the Claude tokenizer saves.
(ablations of text, not valid programs)  Output: structure_share.json"""
import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import io, json, keyword, re, tokenize
import data
from tokcount import count_many


def variants(src):
    v = {"full": src}
    v["no_indent"] = "\n".join(l.lstrip() for l in src.split("\n"))
    v["no_blank_lines"] = "\n".join(l for l in src.split("\n") if l.strip())
    lines = [l for l in src.split("\n") if l.strip()]
    v["no_indent_no_blank"] = "\n".join(l.lstrip() for l in lines)
    # one line per statement but joined with ';' and no indentation: the minimum 'layout' cost
    v["joined_semicolon"] = ";".join(l.strip() for l in lines)
    out = {"ids_lits_ops": None}
    # drop brackets and commas and colons (punctuation)
    v["no_brackets_commas"] = re.sub(r"[()\[\]{},:]", " ", src)
    # keep only identifiers/numbers/strings with a single space: the payload
    names = []
    for tok in tokenize.generate_tokens(io.StringIO(src).readline):
        if tok.type in (tokenize.NAME, tokenize.NUMBER, tokenize.STRING):
            names.append(tok.string)
    v["only_names_lits"] = " ".join(names)
    # keywords only
    kw = [t.string for t in tokenize.generate_tokens(io.StringIO(src).readline) if t.type == tokenize.NAME and keyword.iskeyword(t.string)]
    v["keywords_only"] = " ".join(kw)
    return v


corp = {"ref": data.reference(), "opus": data.model_pairs("opus"), "sonnet": data.model_pairs("sonnet")}
rows, texts = [], []
for cn, c in corp.items():
    for t, vv in c.items():
        py = data.strip_py_comments(vv["python"])
        v = variants(py)
        idx = {}
        for k, s in v.items():
            if s is None:
                continue
            idx[k] = len(texts)
            texts.append(s)
        rows.append((cn, t, idx))
cnt = count_many(texts)
res = [{"corpus": cn, "task": t, **{k: cnt[i] for k, i in idx.items()}} for cn, t, idx in rows]
json.dump(res, open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "structure_share.json"), "w"))
for cn in corp:
    sub = [r for r in res if r["corpus"] == cn]
    f = sum(r["full"] for r in sub)
    print(cn, len(sub), "full", f)
    for k in sub[0]:
        if k in ("corpus", "task", "full"):
            continue
        s = sum(r[k] for r in sub)
        print(f"    {k:22} {s:7}  {100*s/f:5.1f}% of full")
