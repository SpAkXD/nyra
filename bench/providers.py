"""Model providers for the Nyra benchmark (standard library only; SDKs are imported lazily).

A provider turns a conversation into the model's next reply. The runner uses exactly
three things from a provider:

    provider.complete(system, messages, meta) -> Reply
    provider.count_tokens(text)               -> int | None     (optional)
    provider.name / provider.model            (recorded in the result files)

`messages` is a list of {"role": "user" | "assistant", "content": str}. `meta` carries
{"task_id", "lang", "attempt", "sample"}; real providers must ignore it (it exists for
the mock provider and for logging), so the model never sees anything that differs
between languages except the system prompt and the task.

To add a provider (an OpenAI-compatible endpoint, Gemini, ...): subclass `Provider`,
implement `complete` (and `count_tokens` if the API has a token counter), import the SDK
inside `__init__`, and register the class in `PROVIDERS` at the bottom of this file.
"""

from __future__ import annotations

import hashlib
import math
import os
import sys
import threading
import time
from dataclasses import dataclass
from typing import Callable, Optional


class ProviderError(Exception):
    """A provider call failed.

    fatal=True means that every later call would fail the same way (bad key, unknown
    model, rejected parameter), so the runner stops instead of recording model failures.
    """

    def __init__(self, message: str, fatal: bool = False):
        super().__init__(message)
        self.fatal = fatal


@dataclass
class Usage:
    input_tokens: Optional[int] = None
    output_tokens: Optional[int] = None
    estimated: bool = False  # True when the numbers are guesses (mock provider), not API usage

    def to_dict(self) -> dict:
        return {"input_tokens": self.input_tokens, "output_tokens": self.output_tokens, "estimated": self.estimated}


@dataclass
class Reply:
    text: str
    usage: Usage
    stop_reason: Optional[str] = None
    latency_s: float = 0.0
    request_id: Optional[str] = None
    model: Optional[str] = None  # the model id the provider says it served (may differ from the requested alias)


class Provider:
    name = "base"
    default_model = ""
    is_mock = False  # mock results are a pipeline self-test, never a measurement
    tokens_are_estimates = False

    def __init__(self, model: Optional[str] = None, **_options):
        self.model = model or self.default_model

    def ensure_ready(self) -> None:
        """Fail fast (raise ProviderError) if the provider cannot work, before any task is run.
        Not called for --dry-run, so a dry run needs no key and no SDK."""

    def complete(self, system: str, messages: list, meta: dict) -> Reply:
        raise NotImplementedError

    def count_tokens(self, text: str) -> Optional[int]:
        return None

    def describe(self) -> dict:
        return {"name": self.name, "model": self.model}


# --------------------------------------------------------------------------- mock

DEFECTS = ("no_code", "syntax", "runtime", "wrong")


def _estimate_tokens(text: str) -> int:
    return max(1, math.ceil(len(text) / 3.5))


def _fence(lang: str, code: str) -> str:
    return f"```{lang}\n{code.rstrip()}\n```"


class MockProvider(Provider):
    """Replies with the reference solution, so the whole pipeline runs without an API key.

    With `flaky` the first attempt of some tasks is deliberately broken and the repair
    attempt is correct. This exercises the feedback/repair path end to end:

        flaky=True        a deterministic mix of the four defects below (about 4 of 7 tasks)
        flaky="no_code"   reply without a fenced code block
        flaky="syntax"    program that does not parse / compile
        flaky="runtime"   program that crashes (Python); a compile error for Nyra, which has no
                          crashing construct that behaves the same on every platform
        flaky="wrong"     program that prints one extra line first
    """

    name = "mock"
    default_model = "mock"
    is_mock = True
    tokens_are_estimates = True

    def __init__(self, model: Optional[str] = None, *, reference: Optional[Callable[[str, str], Optional[str]]] = None,
                 flaky=False, **_options):
        super().__init__(model)
        if reference is None:
            raise ValueError("MockProvider needs reference=callable(lang, task_id) -> source or None")
        self.reference = reference
        self.flaky = flaky

    def has_reference(self, lang: str, task_id: str) -> bool:
        return self.reference(lang, task_id) is not None

    def defect_for(self, lang: str, task_id: str) -> Optional[str]:
        if not self.flaky:
            return None
        if self.flaky in DEFECTS:
            return self.flaky
        h = int(hashlib.sha256(f"{lang}/{task_id}".encode()).hexdigest(), 16) % 7
        return DEFECTS[h] if h < len(DEFECTS) else None

    def complete(self, system: str, messages: list, meta: dict) -> Reply:
        lang, task_id, attempt = meta["lang"], meta["task_id"], meta["attempt"]
        code = self.reference(lang, task_id)
        if code is None:
            raise ProviderError(f"mock provider has no reference solution for {lang}/{task_id}")
        defect = self.defect_for(lang, task_id) if attempt == 1 else None
        text = _fence(lang, code) if defect is None else self._broken_reply(lang, code, defect)
        prompt_chars = len(system) + sum(len(m["content"]) for m in messages)
        usage = Usage(_estimate_tokens("x" * prompt_chars), _estimate_tokens(text), estimated=True)
        return Reply(text=text, usage=usage, stop_reason="end_turn", latency_s=0.0, model=self.model)

    def count_tokens(self, text: str) -> Optional[int]:
        return _estimate_tokens(text)

    @staticmethod
    def _broken_reply(lang: str, code: str, defect: str) -> str:
        if defect == "no_code":
            return "Here is my solution: I would loop over the numbers and print the results."
        if lang == "nyra":
            if defect == "wrong":
                broken = code.replace("fn main() {", "fn main() {\n    print(12345)", 1)
            elif defect == "syntax":
                broken = "@\n" + code
            else:  # runtime: Nyra has no portable crash, use another compile error (missing `main`)
                broken = code.replace("fn main()", "fn mian()", 1)
        else:
            if defect == "wrong":
                broken = "print(12345)\n" + code
            elif defect == "syntax":
                broken = "def (:\n" + code
            else:
                broken = 'raise RuntimeError("mock failure")\n' + code
        return _fence(lang, broken)


# --------------------------------------------------------------------- Anthropic

# Request fields the SDK accepts as keyword arguments; everything else from --extra-json
# is sent through `extra_body` (the current SDK has no typed sampling parameters).
_ANTHROPIC_FIRST_CLASS = ("thinking", "output_config", "cache_control")


class AnthropicProvider(Provider):
    """Claude through the official `anthropic` SDK (non-streaming Messages API).

    Nothing is sent that the model's defaults do not already decide, except what you
    pass explicitly: `effort` (output_config.effort) and `extra` (a dict of request
    fields, e.g. {"thinking": {"type": "adaptive"}}). Thinking therefore follows the
    model's own default, and `usage.output_tokens` includes any thinking tokens.
    """

    name = "anthropic"
    default_model = "claude-opus-5-5"

    def __init__(self, model: Optional[str] = None, *, max_tokens: int = 16000, effort: Optional[str] = None,
                 extra: Optional[dict] = None, count_tokens: bool = True, client=None, **_options):
        super().__init__(model)
        self.max_tokens = max_tokens
        self.effort = effort
        self.extra = dict(extra or {})
        self._count_enabled = count_tokens
        self._count_baseline: Optional[int] = None
        self._count_lock = threading.Lock()
        self.client = client  # created by ensure_ready() when not injected

    def ensure_ready(self) -> None:
        if self.client is None:
            self.client = self._make_client()

    @staticmethod
    def _make_client():
        key = os.environ.get("ANTHROPIC_API_KEY")
        if not key:
            raise ProviderError("ANTHROPIC_API_KEY is not set (the key is only ever read from that environment variable)",
                                fatal=True)
        try:
            import anthropic  # imported lazily so the mock provider needs no SDK
        except ImportError:
            raise ProviderError("the `anthropic` package is not installed: pip install -r bench/requirements.txt",
                                fatal=True) from None
        # More retries than the default: a benchmark run is long and a single 429/529 should not end it.
        return anthropic.Anthropic(api_key=key, max_retries=6, timeout=600.0)

    def describe(self) -> dict:
        return {"name": self.name, "model": self.model, "max_tokens": self.max_tokens, "effort": self.effort,
                "extra": self.extra}

    def _request(self, system: str, messages: list) -> dict:
        req = {"model": self.model, "max_tokens": self.max_tokens, "system": system, "messages": messages}
        output_config = dict(self.extra.get("output_config") or {})
        if self.effort:
            output_config["effort"] = self.effort
        if output_config:
            req["output_config"] = output_config
        extra_body = {}
        for key, value in self.extra.items():
            if key == "output_config":
                continue
            if key in _ANTHROPIC_FIRST_CLASS:
                req[key] = value
            else:
                extra_body[key] = value
        if extra_body:
            req["extra_body"] = extra_body
        return req

    def complete(self, system: str, messages: list, meta: dict) -> Reply:
        self.ensure_ready()
        start = time.monotonic()
        try:
            resp = self.client.messages.create(**self._request(system, messages))
        except ProviderError:
            raise
        except Exception as exc:  # SDK errors carry .status_code; connection errors do not
            raise _map_error(exc) from exc
        latency = time.monotonic() - start
        text = "".join(getattr(b, "text", "") or "" for b in resp.content if getattr(b, "type", None) == "text")
        u = resp.usage
        input_tokens = sum(int(getattr(u, f, 0) or 0)
                           for f in ("input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens"))
        usage = Usage(input_tokens=input_tokens, output_tokens=int(getattr(u, "output_tokens", 0) or 0))
        return Reply(text=text, usage=usage, stop_reason=getattr(resp, "stop_reason", None), latency_s=latency,
                     request_id=getattr(resp, "_request_id", None), model=getattr(resp, "model", None))

    def _count_raw(self, text: str) -> int:
        self.ensure_ready()
        r = self.client.messages.count_tokens(model=self.model, messages=[{"role": "user", "content": text}])
        return int(r.input_tokens)

    def count_tokens(self, text: str) -> Optional[int]:
        """Tokens of `text` alone, measured with the API's own counter (not an estimate).

        The endpoint counts a whole request, so the fixed per-message overhead is measured
        once with a one-token text ("x") and subtracted. Returns None if counting fails.
        """
        if not self._count_enabled or not text.strip():
            return None
        try:
            with self._count_lock:
                if self._count_baseline is None:
                    self._count_baseline = self._count_raw("x") - 1
            return max(0, self._count_raw(text) - self._count_baseline)
        except Exception as exc:
            self._count_enabled = False
            print(f"warning: token counting disabled after an error: {type(exc).__name__}: {exc}", file=sys.stderr)
            return None


def _map_error(exc: Exception) -> ProviderError:
    status = getattr(exc, "status_code", None)
    detail = getattr(exc, "message", None) or str(exc)
    label = f"{type(exc).__name__}" + (f" (HTTP {status})" if status else "")
    # 400/401/403/404 mean the request itself is wrong (key, model, parameters): every call would fail.
    # So does an error raised by the SDK before anything is sent (a TypeError for an unknown argument,
    # a ValueError such as "streaming is required for this max_tokens"). Everything else (429, 5xx,
    # connection errors) may be transient.
    fatal = status in (400, 401, 403, 404) or (status is None and isinstance(exc, (TypeError, ValueError)))
    return ProviderError(f"{label}: {detail}", fatal=fatal)


# ------------------------------------------------------------------------ registry

PROVIDERS = {"mock": MockProvider, "anthropic": AnthropicProvider}


def make_provider(name: str, model: Optional[str] = None, **options) -> Provider:
    try:
        cls = PROVIDERS[name]
    except KeyError:
        raise ValueError(f"unknown provider {name!r}; available: {', '.join(sorted(PROVIDERS))}") from None
    return cls(model, **options)
