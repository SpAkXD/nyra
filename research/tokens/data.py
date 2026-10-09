"""Loads the paired Nyra/Python corpora used by every measurement script."""
import ast
import io
import json
import re
import tokenize
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SOL = ROOT / "bench" / "solutions"
RES = ROOT / "bench" / "results"
MODELS = {"opus": "2026-10-09-anthropic-claude-opus-5-5.json",
          "sonnet": "2026-10-09-anthropic-claude-sonnet-5-5.json",
          "haiku": "2026-10-09-anthropic-claude-haiku-4-5-20251001.json"}
EXT = {"nyra": "nyra", "python": "py", "typescript": "ts", "rust": "rs"}


def strip_nyra_comments(src):
    out = []
    for line in src.replace("\r\n", "\n").split("\n"):
        i, n, instr = 0, len(line), False
        cut = None
        while i < n:
            c = line[i]
            if instr:
                if c == "\\":
                    i += 2
                    continue
                if c == '"':
                    instr = False
            else:
                if c == '"':
                    instr = True
                elif c == "'":
                    # char literal 'x' or '\x'
                    j = i + 2 if i + 1 < n and line[i + 1] != "\\" else i + 3
                    if j < n and line[j] == "'":
                        i = j + 1
                        continue
                elif c == "/" and i + 1 < n and line[i + 1] == "/":
                    cut = i
                    break
            i += 1
        l = line if cut is None else line[:cut]
        out.append(l.rstrip())
    return squeeze("\n".join(out))


def squeeze(src):
    """strip trailing blank lines, collapse nothing else"""
    lines = src.split("\n")
    # drop lines that were comment-only (they become empty) but keep intentional single blank lines
    res = []
    for l in lines:
        if l == "" and res and res[-1] == "":
            continue
        res.append(l)
    return "\n".join(res).strip("\n")


def strip_py_comments(src):
    src = src.replace("\r\n", "\n")
    try:
        tree = ast.parse(src)
    except SyntaxError:
        return squeeze(src)
    drop = set()
    for node in ast.walk(tree):
        if isinstance(node, (ast.Module, ast.FunctionDef, ast.ClassDef, ast.AsyncFunctionDef)):
            body = node.body
            if body and isinstance(body[0], ast.Expr) and isinstance(getattr(body[0], "value", None), ast.Constant) \
                    and isinstance(body[0].value.value, str):
                for ln in range(body[0].lineno, body[0].end_lineno + 1):
                    drop.add(ln)
    lines = src.split("\n")
    cuts = {}
    for tok in tokenize.generate_tokens(io.StringIO(src).readline):
        if tok.type == tokenize.COMMENT:
            cuts[tok.start[0]] = tok.start[1]
    out = []
    for i, l in enumerate(lines, 1):
        if i in drop:
            continue
        if i in cuts:
            l = l[:cuts[i]]
        out.append(l.rstrip())
    return squeeze("\n".join(out))


def strip_c_comments(src):  # ts / rust, // only (and /* */ rare)
    src = re.sub(r"/\*.*?\*/", "", src.replace("\r\n", "\n"), flags=re.S)
    return strip_nyra_comments(src)


STRIP = {"nyra": strip_nyra_comments, "python": strip_py_comments,
         "typescript": strip_c_comments, "rust": strip_c_comments}


def reference():
    """{task: {lang: code}} from bench/solutions"""
    tasks = {}
    for lang, ext in EXT.items():
        for p in sorted((SOL / lang).glob("*." + ext)):
            tasks.setdefault(p.stem, {})[lang] = p.read_text(encoding="utf-8").replace("\r\n", "\n").strip("\n")
    return {t: v for t, v in tasks.items() if len(v) == 4}


def model_runs(model):
    """{task: {lang: (code, first_try_pass)}} first attempts of a model"""
    d = json.loads((RES / MODELS[model]).read_text(encoding="utf-8"))
    out = {}
    for r in d["records"]:
        if not r["attempts"]:
            continue
        a = r["attempts"][0]
        out.setdefault(r["task_id"], {})[r["lang"]] = ((a["code"] or "").replace("\r\n", "\n").strip("\n"),
                                                         bool(r["first_try"]), a.get("code_tokens"))
    return out


def model_pairs(model, require_all=False):
    """tasks where Nyra AND Python passed on the first try -> {task: {lang: code}}"""
    runs = model_runs(model)
    res = {}
    for t, v in runs.items():
        if "nyra" in v and "python" in v and v["nyra"][1] and v["python"][1]:
            if require_all and not all(l in v and v[l][1] for l in EXT):
                continue
            res[t] = {l: x[0] for l, x in v.items()}
    return res


if __name__ == "__main__":
    ref = reference()
    print("reference tasks", len(ref))
    for m in MODELS:
        print(m, len(model_pairs(m)))
