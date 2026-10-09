"""Do keyword spellings matter?  Token cost of the same snippet with alternative keywords (Claude tokenizer)."""
import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import json
from tokcount import count_many

BODY = '''{fn} area(r: Rect) {arrow} {{
    {let} w = r.w
    {var} h = r.h
    {ret} w * h
}}

{fn} twice(n: int) {arrow} {{
    {ret} n * 2
}}
'''
def mk(fn="fn", let="let", var="var", ret="ret", arrow="-> int"):
    return BODY.format(fn=fn, let=let, var=var, ret=ret, arrow=arrow)

variants = {
 "current: fn let var ret": mk(),
 "def let var return": mk(fn="def", ret="return"),
 "def let var ret": mk(fn="def", ret="ret"),
 "fn let var return": mk(ret="return"),
 "func let var return": mk(fn="func", ret="return"),
 "fun val var return": mk(fn="fun", let="val", ret="return"),
 "fn const let return (JS)": mk(let="const", var="let", ret="return"),
 "fn val var ret": mk(let="val"),
 "fn final var ret": mk(let="final"),
 "fn let mut ret": mk(var="mut"),
 "fn let var ret (arrow ':')": mk(arrow=":"),
}
txt = list(variants.values())
c = count_many(txt)
base = c[0]
for (k, v), n in zip(variants.items(), c):
    print(f"{n:4} ({n-base:+d})  {k}")
