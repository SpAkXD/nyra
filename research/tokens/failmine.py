"""Compile errors of Nyra attempts in the 2026-10-09 benchmark (all attempts of the 3 models), grouped by code and normalized message.
Shows which Python/TS habits models still bring to Nyra.  Output: failmine.json"""
import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import json, re, collections
import data
cnt = collections.Counter(); ex = {}
by_model = collections.defaultdict(collections.Counter)
tot_att = collections.Counter(); bad_att = collections.Counter()
for m, f in data.MODELS.items():
    d = json.load(open(data.RES / f, encoding="utf-8"))
    for r in d["records"]:
        if r["lang"] != "nyra":
            continue
        for a in r["attempts"]:
            tot_att[m] += 1
            res = a.get("result") or {}
            errs = res.get("errors") or []
            if res.get("kind") not in ("pass",) and errs:
                bad_att[m] += 1
            for e in errs:
                msg = e.get("message", "") if isinstance(e, dict) else str(e)
                code = e.get("code", "?") if isinstance(e, dict) else "?"
                key = (code, re.sub(r"`[^`]*`", "`_`", msg)[:90])
                cnt[key] += 1; by_model[m][key] += 1
                ex.setdefault(key, (m, r["task_id"], msg[:160]))
print({m: (tot_att[m], bad_att[m]) for m in tot_att})
for (code, msg), n in cnt.most_common(28):
    print(f"{n:4} {code} {msg}   e.g. {ex[(code,msg)][2]!r}")
json.dump([{"code": k[0], "msg": k[1], "n": n, "example": ex[k][2]} for k, n in cnt.most_common(60)], open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "failmine.json"), "w"), indent=1)
