"""Token size of the spec material and of the exact system prompt bench/run.py sends (Nyra / Python), per model."""
import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import json, importlib.util
from pathlib import Path
import anthropic
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "bench"))
spec_ = importlib.util.spec_from_file_location("benchrun", ROOT / "bench" / "run.py")
run = importlib.util.module_from_spec(spec_); sys.modules["benchrun"] = run
spec_.loader.exec_module(run)
client = anthropic.Anthropic(max_retries=6)

def rd(p): return Path(p).read_text(encoding="utf-8").replace("\r\n", "\n")

def count_req(model, system, user):
    kw = dict(model=model, messages=[{"role": "user", "content": user}])
    if system: kw["system"] = system
    return client.messages.count_tokens(**kw).input_tokens

task = json.loads((ROOT / "bench/tasks/ackermann.json").read_text(encoding="utf-8"))["prompt"]
spec = rd(ROOT / "docs/SPEC.md")
nyra_sys = run._NYRA_SYSTEM.format(spec=spec.strip())
out = {}
for model in ["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-4-5-20251001"]:
    base = count_req(model, None, "x") - 1
    r = {
      "nyra_request_input_tokens": count_req(model, nyra_sys, task),
      "python_request_input_tokens": count_req(model, run._PYTHON_SYSTEM, task),
      "task_only": count_req(model, None, task) - base,
      "SPEC.md": count_req(model, None, spec) - base,
      "AI_GUIDE.md": count_req(model, None, rd(ROOT / "docs/AI_GUIDE.md")) - base,
      "llms.txt": count_req(model, None, rd(ROOT / "llms.txt")) - base,
      "ERRORS.md": count_req(model, None, rd(ROOT / "docs/ERRORS.md")) - base,
    }
    out[model] = r
    print(model, r)
json.dump(out, open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "spec_size.json"), "w"), indent=1)
