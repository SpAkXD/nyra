"""Token size of SPEC-agent.md versus SPEC.md, and the full system prompt bench/run.py would send with it."""
import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import json, importlib.util
from pathlib import Path
import anthropic
ROOT = Path(__file__).resolve().parents[2]
spec_ = importlib.util.spec_from_file_location("benchrun", ROOT / "bench" / "run.py")
run = importlib.util.module_from_spec(spec_); sys.modules["benchrun"] = run; spec_.loader.exec_module(run)
client = anthropic.Anthropic(max_retries=6)
task = json.loads((ROOT / "bench/tasks/ackermann.json").read_text(encoding="utf-8"))["prompt"]
def rd(p): return Path(p).read_text(encoding="utf-8").replace("\r\n", "\n")
files = {"SPEC.md (current)": ROOT / "docs/SPEC.md", "SPEC-agent.md (draft)": Path(__file__).parent / "SPEC-agent.md"}
for model in ["claude-opus-5-5", "claude-haiku-4-5-20251001"]:
    base = client.messages.count_tokens(model=model, messages=[{"role": "user", "content": "x"}]).input_tokens - 1
    for k, p in files.items():
        s = rd(p)
        sysm = run._NYRA_SYSTEM.format(spec=s.strip())
        full = client.messages.count_tokens(model=model, system=sysm, messages=[{"role": "user", "content": task}]).input_tokens
        alone = client.messages.count_tokens(model=model, messages=[{"role": "user", "content": s}]).input_tokens - base
        print(f"{model:28} {k:24} {len(s):6} chars  spec {alone:5} tok   full Nyra request {full:5} tok")
