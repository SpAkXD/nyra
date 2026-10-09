"""Cost of the Nyra requests of the 2026-10-09 benchmark with the current spec, a cached spec, a compressed spec, and both.
Uses bench/providers.py list prices (Opus 5.5 5/25, Sonnet 5.5 3/15, Haiku 4.5 1/5 $/M) and the documented cache multipliers
(read 0.1x, 5-minute write 1.25x, 1-hour write 2x).  The Opus 5.5 / Sonnet 5.5 rates in the API docs differ (see report)."""
import json
from pathlib import Path
ROOT = Path(__file__).resolve().parents[2]
PRICE = {"opus-5-5": (5.0, 25.0), "sonnet-5-5": (3.0, 15.0), "haiku-4-5-20251001": (1.0, 5.0)}
# system-prompt tokens of the Nyra request (measured with count_tokens): current spec, and SPEC-agent.md draft
SYS_NOW = {"opus-5-5": 8471 - 147, "sonnet-5-5": 8471 - 147, "haiku-4-5-20251001": 7162 - 140}
SYS_NEW = {"opus-5-5": 3790 - 147, "sonnet-5-5": 3790 - 147, "haiku-4-5-20251001": 3189 - 140}
R, W5, W1 = 0.1, 1.25, 2.0
MIN_CACHE = {"opus-5-5": 512, "sonnet-5-5": 512, "haiku-4-5-20251001": 4096}  # minimum cacheable prefix (API docs)
out = {}
print(f"{'model':20} {'scenario':34} {'Nyra $/run':>10} {'vs now':>7} {'Nyra $/req':>10}")
for m, (pi, po) in PRICE.items():
    d = json.load(open(ROOT / f"bench/results/2026-10-09-anthropic-claude-{m}.json", encoding="utf-8"))
    n = ti = to = 0
    py_ti = py_to = py_n = 0
    for r in d["records"]:
        for a in r["attempts"]:
            u = a["usage"]
            if r["lang"] == "nyra":
                n += 1; ti += u["input_tokens"] or 0; to += u["output_tokens"] or 0
            elif r["lang"] == "python":
                py_n += 1; py_ti += u["input_tokens"] or 0; py_to += u["output_tokens"] or 0
    out_cost = to * po / 1e6
    def cost(sys_tokens, cached, writes=7):
        # input tokens that are not the system prompt (task, repairs) stay as they are
        non_sys = ti - n * SYS_NOW[m]
        if cached and sys_tokens < MIN_CACHE[m]:
            cached = False  # below the minimum: a marker silently caches nothing
        if not cached:
            inp = (non_sys + n * sys_tokens) * pi / 1e6
        else:
            inp = (non_sys * pi + writes * sys_tokens * pi * W5 + (n - writes) * sys_tokens * pi * R) / 1e6
        return inp + out_cost
    base = cost(SYS_NOW[m], False)
    for name, st, c in [("now (uncached, SPEC.md)", SYS_NOW[m], False), ("SPEC.md + 5-min cache", SYS_NOW[m], True),
                        ("SPEC-agent.md, uncached", SYS_NEW[m], False), ("SPEC-agent.md + 5-min cache", SYS_NEW[m], True)]:
        t = cost(st, c)
        print(f"{m:20} {name:34} {t:10.2f} {100*(t/base-1):+6.0f}% {t/n:10.4f}")
        out.setdefault(m, {})[name] = {"run": t, "req": t / n}
    print(f"{'':20} Nyra requests {n}, mean input {ti/n:.0f} tok, mean output {to/n:.0f} tok; Python request mean {py_ti/py_n:.0f} in / {py_to/py_n:.0f} out = ${(py_ti*pi+py_to*po)/py_n/1e6:.4f}")
json.dump(out, open(Path(__file__).parent / "caching_cost.json", "w"), indent=1)
