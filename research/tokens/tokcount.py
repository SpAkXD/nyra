"""Cached, parallel token counting with the Anthropic count_tokens endpoint (free, no generation).

    from tokcount import count, count_many
    count("print(1)")                  # tokens of the text alone (per-message overhead subtracted)
    count_many(["a", "b"])             # list of ints, threaded

Counts are cached in research/tokens/cache_<model>.json keyed by sha1(text). Needs ANTHROPIC_API_KEY in the
environment (run through a wrapper that loads it; this module never prints, logs or stores the key).
"""
import hashlib
import json
import os
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

MODEL = os.environ.get("TOK_MODEL", "claude-sonnet-5-5")
HERE = Path(__file__).parent
CACHE_PATH = HERE / f"cache_{MODEL}.json"
_lock = threading.Lock()
_cache = json.loads(CACHE_PATH.read_text(encoding="utf-8")) if CACHE_PATH.exists() else {}
_client = None
_baseline = None
_dirty = 0


def _cl():
    global _client
    if _client is None:
        import anthropic
        _client = anthropic.Anthropic(max_retries=6)
    return _client


def _raw(text):
    r = _cl().messages.count_tokens(model=MODEL, messages=[{"role": "user", "content": text}])
    return int(r.input_tokens)


def _base():
    global _baseline
    if _baseline is None:
        _baseline = _raw("x") - 1
    return _baseline


def save():
    with _lock:
        CACHE_PATH.write_text(json.dumps(_cache), encoding="utf-8")


def count(text):
    global _dirty
    if not text.strip():
        return 0
    k = hashlib.sha1(text.encode("utf-8")).hexdigest()
    if k in _cache:
        return _cache[k]
    b = _base()
    n = max(0, _raw(text) - b)
    with _lock:
        _cache[k] = n
        _dirty += 1
        if _dirty >= 50:
            _dirty = 0
            CACHE_PATH.write_text(json.dumps(_cache), encoding="utf-8")
    return n


def count_many(texts, workers=8):
    _base()
    with ThreadPoolExecutor(workers) as ex:
        out = list(ex.map(count, texts))
    save()
    return out


if __name__ == "__main__":
    t = time.time()
    print(count("print(1)"), count("fn main() {\n    print(1)\n}"), "baseline", _base(), f"{time.time()-t:.1f}s")
    save()
