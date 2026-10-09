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

Providers: `mock` (replays the reference solutions), `anthropic` (the official SDK, one
model family) and `openrouter` (one HTTPS endpoint, hundreds of models; urllib only).

To add a provider (Gemini, ...): subclass `Provider`, implement `complete` (and
`count_tokens` if the API has a token counter), import the SDK inside `__init__`, and
register the class in `PROVIDERS` at the bottom of this file.
"""

from __future__ import annotations

import hashlib
import http.client
import ipaddress
import json
import math
import os
import random
import re
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from typing import Callable, Optional


class ProviderError(Exception):
    """A provider call failed.

    fatal=True means that every later call of this model would fail the same way (unknown
    model, rejected parameter), so the runner stops that model instead of recording model
    failures. stop_all=True (implies fatal) means that every model would fail, because the
    key or the account is the problem (invalid key, no credits): the whole run stops.
    """

    def __init__(self, message: str, fatal: bool = False, stop_all: bool = False):
        super().__init__(message)
        self.fatal = fatal or stop_all
        self.stop_all = stop_all


@dataclass
class Usage:
    input_tokens: Optional[int] = None
    output_tokens: Optional[int] = None  # as billed: includes any thinking / reasoning tokens
    estimated: bool = False  # True when the numbers are guesses (mock provider), not API usage
    reasoning_tokens: Optional[int] = None  # the part of output_tokens spent thinking, when the API says so
    cost: Optional[float] = None  # what the API charged for this call in US dollars, when it says so

    def to_dict(self) -> dict:
        out = {"input_tokens": self.input_tokens, "output_tokens": self.output_tokens, "estimated": self.estimated}
        if self.reasoning_tokens is not None:
            out["reasoning_tokens"] = self.reasoning_tokens
        if self.cost is not None:
            out["cost_usd"] = self.cost
        return out


@dataclass
class Reply:
    text: str
    usage: Usage
    stop_reason: Optional[str] = None
    latency_s: float = 0.0
    request_id: Optional[str] = None
    model: Optional[str] = None  # the model id the provider says it served (may differ from the requested alias)
    upstream: Optional[str] = None  # who actually ran the model, when a router says so (OpenRouter: "Anthropic")


class Provider:
    name = "base"
    default_model = ""
    is_mock = False  # mock results are a pipeline self-test, never a measurement
    tokens_are_estimates = False
    count_all_attempts = True  # False when count_tokens costs money: the runner then counts first attempts only

    def __init__(self, model: Optional[str] = None, **_options):
        self.model = model or self.default_model

    def ensure_ready(self) -> None:
        """Fail fast (raise ProviderError) if the provider cannot work, before any task is run.
        Not called for --dry-run, so a dry run needs no key and no SDK."""

    def complete(self, system: str, messages: list, meta: dict) -> Reply:
        raise NotImplementedError

    def count_tokens(self, text: str) -> Optional[int]:
        return None

    def spent(self) -> Optional[float]:
        """Dollars charged so far by this provider instance, if the API reports costs (else None)."""
        return None

    def describe(self) -> dict:
        return {"name": self.name, "model": self.model}


# --------------------------------------------------------------------------- mock

DEFECTS = ("no_code", "syntax", "runtime", "wrong")
# Not part of the "mix": `hardcode` replies with a program that ignores its input and prints the example's answer. It only
# makes sense for tasks with hidden inputs (the v2 tier), where it must pass the example and fail the hidden inputs.
EXTRA_DEFECTS = ("hardcode",)


def _estimate_tokens(text: str) -> int:
    return max(1, math.ceil(len(text) / 3.5))


def _fence(lang: str, code: str) -> str:
    return f"```{lang}\n{code.rstrip()}\n```"


def hardcoded_program(lang: str, text: str) -> str:
    """A program that ignores its input and prints `text`: what a model that memorizes the example would write."""
    if lang == "python":
        return "import sys\nsys.stdout.write(" + repr(text) + ")\n"
    if lang == "typescript":
        return "process.stdout.write(" + json.dumps(text) + ");\n"
    if lang == "rust":
        return "fn main() {\n    print!(\"{}\", " + json.dumps(text) + ");\n}\n"
    quoted = (text.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n")
              .replace("{", "{{").replace("}", "}}"))
    return 'fn main() {\n    print("' + quoted + '", end: "")\n}\n'


class MockProvider(Provider):
    """Replies with the reference solution, so the whole pipeline runs without an API key.

    With `flaky` the first attempt of some tasks is deliberately broken and the repair
    attempt is correct. This exercises the feedback/repair path end to end:

        flaky=True        a deterministic mix of the four defects below (about 4 of 7 tasks)
        flaky="no_code"   reply without a fenced code block
        flaky="syntax"    program that does not parse / compile
        flaky="runtime"   program that crashes (Python, TypeScript, Rust); a compile error for
                          Nyra, which has no crashing construct that behaves the same on every platform
        flaky="wrong"     program that prints one extra line first

    The model id can ask for the same thing, so a multi-model mock run (`--models mock,mock-flaky,
    mock-wrong`) produces different "models" for the comparison tables: `mock` is perfect,
    `mock-flaky` is the mix and `mock-<defect>` is that defect. `--mock-flaky` applies to every model.
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
        if not flaky and self.model != "mock":
            variant = self.model[len("mock-"):] if self.model.startswith("mock-") else None
            if variant == "flaky":
                flaky = True
            elif variant in DEFECTS or variant in EXTRA_DEFECTS:
                flaky = variant
            else:
                raise ValueError(f"unknown mock model {self.model!r}; use mock, mock-flaky or "
                                 f"mock-<{'|'.join(DEFECTS + EXTRA_DEFECTS)}>")
        self.flaky = flaky

    def has_reference(self, lang: str, task_id: str) -> bool:
        return self.reference(lang, task_id) is not None

    def defect_for(self, lang: str, task_id: str) -> Optional[str]:
        if not self.flaky:
            return None
        if self.flaky in DEFECTS or self.flaky in EXTRA_DEFECTS:
            return self.flaky
        h = int(hashlib.sha256(f"{lang}/{task_id}".encode()).hexdigest(), 16) % 7
        return DEFECTS[h] if h < len(DEFECTS) else None

    def complete(self, system: str, messages: list, meta: dict) -> Reply:
        lang, task_id, attempt = meta["lang"], meta["task_id"], meta["attempt"]
        code = self.reference(lang, task_id)
        if code is None:
            raise ProviderError(f"mock provider has no reference solution for {lang}/{task_id}")
        defect = self.defect_for(lang, task_id) if attempt == 1 else None
        example = meta.get("example_output")
        if defect == "hardcode" and example is None:
            defect = None  # a task without hidden inputs has nothing to hard-code
        if defect == "hardcode":
            text = _fence(lang, hardcoded_program(lang, example))
        else:
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
        elif lang == "rust":
            if defect == "wrong":
                broken = code.replace("fn main() {", 'fn main() {\n    println!("12345");', 1)
            elif defect == "syntax":
                broken = "@\n" + code
            else:
                broken = code.replace("fn main() {", 'fn main() {\n    panic!("mock failure");', 1)
        elif lang == "typescript":
            if defect == "wrong":
                broken = "console.log(12345);\n" + code
            elif defect == "syntax":
                broken = "const = ;\n" + code
            else:
                broken = 'throw new Error("mock failure");\n' + code
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


# US dollars per million tokens (input, output), for the cost the API does not report itself.
ANTHROPIC_PRICES = {
    "claude-opus-5-5": (5.0, 25.0),
    "claude-sonnet-5-5": (3.0, 15.0),
    "claude-haiku-4-5-20251001": (1.0, 5.0),
}


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
        self._spent_lock = threading.Lock()
        self._spent = 0.0
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
        # A key that is not scoped to a workspace needs the workspace id (not a secret) in a header.
        headers = {}
        if os.environ.get("ANTHROPIC_WORKSPACE_ID"):
            headers["anthropic-workspace-id"] = os.environ["ANTHROPIC_WORKSPACE_ID"].strip()
        return anthropic.Anthropic(api_key=key, max_retries=6, timeout=600.0, default_headers=headers or None)

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
        price = ANTHROPIC_PRICES.get(self.model)
        if price is not None:  # computed from the token counts at list price (no caching is used)
            usage.cost = (input_tokens * price[0] + usage.output_tokens * price[1]) / 1e6
            with self._spent_lock:
                self._spent += usage.cost
        return Reply(text=text, usage=usage, stop_reason=getattr(resp, "stop_reason", None), latency_s=latency,
                     request_id=getattr(resp, "_request_id", None), model=getattr(resp, "model", None))

    def spent(self) -> Optional[float]:
        return self._spent if self.model in ANTHROPIC_PRICES else None

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


# ------------------------------------------------------------------------ HTTP (urllib)

# transport(method, url, headers, body_bytes_or_None, timeout) -> (status, response_headers, body_bytes)
# It must return normally for every HTTP status (4xx and 5xx included) and raise OSError or
# http.client.HTTPException for connection problems, timeouts and TLS errors. Tests replace it.
Transport = Callable[[str, str, dict, Optional[bytes], float], tuple]


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    """Never follow a redirect: urllib would send the Authorization header to wherever it points."""

    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def urllib_transport(method: str, url: str, headers: dict, body: Optional[bytes], timeout: float) -> tuple:
    request = urllib.request.Request(url, data=body, headers=headers, method=method)
    opener = urllib.request.build_opener(_NoRedirect)
    try:
        with opener.open(request, timeout=timeout) as resp:
            return resp.status, dict(resp.headers.items()), resp.read()
    except urllib.error.HTTPError as exc:  # 3xx (not followed), 4xx, 5xx: the caller decides what they mean
        try:
            data = exc.read()
        except (OSError, http.client.HTTPException):
            data = b""
        return exc.code, dict(exc.headers.items()) if exc.headers else {}, data


def loads_json(raw) -> object:
    """Parse a response body. OpenRouter may send whitespace first to keep a long request alive; returns
    None when the body is not JSON."""
    if isinstance(raw, (bytes, bytearray)):
        raw = bytes(raw).decode("utf-8", "replace")
    try:
        return json.loads(raw.strip())
    except ValueError:
        return None


def _is_loopback(host: str) -> bool:
    if host == "localhost" or host.endswith(".localhost"):
        return True
    try:
        return ipaddress.ip_address(host.strip("[]")).is_loopback
    except ValueError:
        return False


def check_base_url(url: str) -> str:
    """The API key travels in a header, so only https (or plain http to this machine, for tests) is allowed."""
    parts = urllib.parse.urlsplit(url)
    host = (parts.hostname or "").lower()
    if not host or not (parts.scheme == "https" or (parts.scheme == "http" and _is_loopback(host))):
        raise ValueError(f"base URL {url!r} must be https:// (plain http is only allowed for localhost)")
    return url.rstrip("/")


# ---------------------------------------------------------------------- OpenRouter

OPENROUTER_BASE_URL = "https://openrouter.ai/api/v1"
OPENROUTER_KEY_ENV = "OPENROUTER_API_KEY"
_APP_NAME = "Nyra benchmark"  # optional attribution headers: they name the app on openrouter.ai, nothing else
_APP_URL = "https://github.com/SpAkXD/nyra"
_USER_AGENT = "nyra-bench/1 (+https://github.com/SpAkXD/nyra)"

# Worth retrying: request timeouts, rate limits, and server trouble (502 = the model's providers are down or
# returned garbage, 503 = no provider fits the request, 52x = Cloudflare in front of OpenRouter).
_RETRY_STATUSES = frozenset({408, 425, 429}) | frozenset(range(500, 600))
_MODEL_FATAL = frozenset({400, 404, 422})  # the request itself is wrong: every call for this model fails the same way
_ACCOUNT_FATAL = frozenset({401, 402})  # bad key or no credits: every model fails
# OpenRouter normalizes finish_reason; the report speaks Anthropic ("max_tokens" means the budget ran out).
_STOP_REASONS = {"stop": "end_turn", "length": "max_tokens", "content_filter": "refusal", "tool_calls": "tool_use"}
_RESERVED_BODY_KEYS = ("model", "messages", "stream")
COUNT_MAX_TOKENS = 16  # the echo request used to count tokens may generate this much (some APIs reject less)


def _number(value) -> Optional[float]:
    return value if isinstance(value, (int, float)) and not isinstance(value, bool) else None


_INLINE_THINKING = re.compile(r"\A\s*<think(?:ing)?>.*?</think(?:ing)?>\s*", re.S)


def _text_of(content) -> str:
    """message.content is a string; some providers send a list of {"type": "text", "text": ...} parts. A reply that
    starts with an inline <think>...</think> block (some open models, when the provider does not split it off into
    message.reasoning) loses it: the drafts in there contain code blocks that are not the answer."""
    if isinstance(content, list):
        content = "".join(part.get("text", "") for part in content
                          if isinstance(part, dict) and isinstance(part.get("text"), str))
    if not isinstance(content, str):
        return ""
    return _INLINE_THINKING.sub("", content, count=1)


def usage_from_openrouter(u) -> Usage:
    """OpenRouter always includes usage: prompt_tokens / completion_tokens counted with the model's own
    tokenizer, completion_tokens_details.reasoning_tokens (inside completion_tokens), and cost in dollars."""
    if not isinstance(u, dict):
        return Usage()
    details = u.get("completion_tokens_details")
    reasoning = _number(details.get("reasoning_tokens")) if isinstance(details, dict) else None
    inp, out, cost = _number(u.get("prompt_tokens")), _number(u.get("completion_tokens")), _number(u.get("cost"))
    return Usage(input_tokens=None if inp is None else int(inp), output_tokens=None if out is None else int(out),
                 reasoning_tokens=None if reasoning is None else int(reasoning),
                 cost=None if cost is None else float(cost))


def _upstream_provider(parsed: dict) -> Optional[str]:
    """Who ran the model: the selected endpoint of `openrouter_metadata` (asked for with a header), or a top-level
    `provider` string that some responses carry."""
    meta = parsed.get("openrouter_metadata")
    endpoints = meta.get("endpoints") if isinstance(meta, dict) else None
    available = endpoints.get("available") if isinstance(endpoints, dict) else None
    for endpoint in available if isinstance(available, list) else []:
        if isinstance(endpoint, dict) and endpoint.get("selected") and isinstance(endpoint.get("provider"), str):
            return endpoint["provider"]
    provider = parsed.get("provider")
    return provider if isinstance(provider, str) and provider else None


def _error_object(parsed) -> Optional[dict]:
    """The error OpenRouter reports in a body: top level (`{"error": {...}}`) or on the first choice."""
    if not isinstance(parsed, dict):
        return None
    if isinstance(parsed.get("error"), dict):
        return parsed["error"]
    choices = parsed.get("choices")
    if isinstance(choices, list) and choices and isinstance(choices[0], dict):
        first = choices[0]
        if isinstance(first.get("error"), dict):
            return first["error"]
        if first.get("finish_reason") == "error":
            return {"message": "the provider stopped with finish_reason=error"}
    return None


def _error_code(error: Optional[dict]) -> Optional[int]:
    code = error.get("code") if error else None
    if isinstance(code, str) and code.isdigit():
        code = int(code)
    return code if isinstance(code, int) and not isinstance(code, bool) else None


def _failure_text(status: int, error: Optional[dict], raw: bytes) -> str:
    if error is not None:
        text = str(error.get("message") or "").strip() or "unknown error"
        meta = error.get("metadata")
        if isinstance(meta, dict) and (meta.get("provider_name") or meta.get("raw")):
            text += f" [{meta.get('provider_name') or 'provider'}: {str(meta.get('raw') or '')[:200]}]"
        return f"HTTP {status}: {text}"
    snippet = re.sub(r"\s+", " ", raw.decode("utf-8", "replace")).strip()[:200]
    return f"HTTP {status}" + (f": {snippet}" if snippet else "")


def _retry_after(headers: dict) -> Optional[float]:
    value = headers.get("retry-after")
    if value is None:
        return None
    try:
        return max(0.0, float(value))
    except ValueError:
        return None  # an HTTP date: not worth parsing, the exponential backoff covers it


class OpenRouterProvider(Provider):
    """Any model on OpenRouter through its OpenAI-compatible chat completions endpoint (non-streaming).

    Standard library only (urllib). The key comes from the environment variable OPENROUTER_API_KEY and
    nowhere else: not from an argument, not from a file. Model ids are always given explicitly
    (`anthropic/claude-opus-5.5`; `python bench/models.py` lists the current ones).

    Nothing is sent that the model's own defaults already decide, except what you pass explicitly:
    `effort` (becomes reasoning.effort) and `extra` (more top-level request fields, e.g.
    {"reasoning": {"max_tokens": 2000}} or {"provider": {"order": ["anthropic"], "allow_fallbacks": false}}).
    Many models think by default; `usage.completion_tokens` (the billed output) then includes the thinking and
    `usage.completion_tokens_details.reasoning_tokens` says how much of it that was.

    Failures: connection errors, timeouts, 408/425/429 and 5xx (also when OpenRouter reports them inside a
    200 response) are retried with exponential backoff that honours Retry-After; a bad request (400, 404,
    422) stops that model, a bad key or no credits (401, 402) stops the whole run.

    Code tokens: OpenRouter has no token-counting endpoint, so `count_tokens` sends the code alone as a
    user message with max_tokens=16 and reads usage.prompt_tokens, minus the same measurement of "x" (the
    fixed per-request overhead). That is billed, so the runner counts first attempts only.
    """

    name = "openrouter"
    default_model = ""  # never guessed: the caller names the models
    count_all_attempts = False

    def __init__(self, model: Optional[str] = None, *, max_tokens: int = 16000, effort: Optional[str] = None,
                 extra: Optional[dict] = None, count_tokens: bool = True, base_url: str = OPENROUTER_BASE_URL,
                 timeout: float = 900.0, max_retries: int = 6, backoff: float = 1.0, max_backoff: float = 60.0,
                 jitter: bool = True, max_consecutive_failures: int = 10, transport: Optional[Transport] = None,
                 sleep: Callable[[float], None] = time.sleep, **_options):
        super().__init__(model)
        self.max_tokens = max_tokens
        self.effort = effort
        self.extra = dict(extra or {})
        bad = [k for k in _RESERVED_BODY_KEYS if k in self.extra]
        if bad:
            raise ValueError(f"--extra-json may not set {', '.join(bad)}: the runner owns those request fields")
        self.base_url = check_base_url(base_url)
        self.timeout = timeout
        self.max_retries = max_retries
        self.backoff = backoff
        self.max_backoff = max_backoff
        self.jitter = jitter
        self.max_consecutive_failures = max_consecutive_failures
        self.transport: Transport = transport or urllib_transport
        self._sleep = sleep
        self._key: Optional[str] = None
        self._lock = threading.Lock()
        self._failures = 0  # consecutive calls that failed even after their retries
        self._spent: Optional[float] = None
        self._count_enabled = count_tokens
        self._count_baseline: Optional[int] = None
        self._count_cache: dict = {}
        self._count_failures = 0

    # ---- setup

    def ensure_ready(self) -> None:
        if not self.model:
            raise ProviderError("no model given: pass --models with OpenRouter model ids (python bench/models.py "
                                "lists them) or --models default", fatal=True)
        if self._key is None:
            key = os.environ.get(OPENROUTER_KEY_ENV, "").strip()
            if not key:
                raise ProviderError(f"{OPENROUTER_KEY_ENV} is not set (the key is only ever read from that "
                                    "environment variable)", stop_all=True)
            if not key.isascii() or any(c.isspace() for c in key):
                raise ProviderError(f"{OPENROUTER_KEY_ENV} contains characters that cannot be part of an API key "
                                    "(quotes or spaces around it?)", stop_all=True)
            self._key = key

    def describe(self) -> dict:
        return {"name": self.name, "model": self.model, "base_url": self.base_url, "max_tokens": self.max_tokens,
                "effort": self.effort, "extra": self.extra}

    def spent(self) -> Optional[float]:
        with self._lock:
            return self._spent

    # ---- one request

    def _headers(self) -> dict:
        # X-OpenRouter-Metadata asks for `openrouter_metadata` in the response, which says which upstream provider
        # ran the model (OpenRouter load-balances between providers; a result file should say which one it was).
        return {"Authorization": f"Bearer {self._key}", "Content-Type": "application/json",
                "Accept": "application/json", "User-Agent": _USER_AGENT, "HTTP-Referer": _APP_URL,
                "X-Title": _APP_NAME, "X-OpenRouter-Metadata": "enabled"}

    def _request_body(self, system: str, messages: list) -> dict:
        body = {"model": self.model, "max_tokens": self.max_tokens,
                "messages": [{"role": "system", "content": system}, *messages]}
        reasoning = dict(self.extra.get("reasoning") or {})
        if self.effort:
            reasoning["effort"] = self.effort
        body.update({k: v for k, v in self.extra.items() if k != "reasoning"})
        if reasoning:
            body["reasoning"] = reasoning
        return body

    def _scrub(self, text: str) -> str:
        return text.replace(self._key, "***") if self._key else text

    def _delay(self, attempt: int, retry_after: Optional[float]) -> float:
        if retry_after is not None:
            return min(retry_after, 2 * self.max_backoff)
        delay = min(self.max_backoff, self.backoff * 2 ** attempt)
        return delay * random.uniform(0.75, 1.25) if self.jitter else delay

    def _post(self, body: dict) -> tuple:
        """POST /chat/completions, retrying what is worth retrying. Returns (parsed JSON, seconds the
        successful request took) or raises ProviderError."""
        url = self.base_url + "/chat/completions"
        data = json.dumps(body, ensure_ascii=False).encode("utf-8")
        headers = self._headers()
        error: Optional[ProviderError] = None
        for attempt in range(self.max_retries + 1):
            retry_after = None
            started = time.monotonic()
            try:
                status, resp_headers, raw = self.transport("POST", url, headers, data, self.timeout)
            except (OSError, http.client.HTTPException) as exc:  # includes socket timeouts and TLS errors
                error = ProviderError(f"{type(exc).__name__}: {self._scrub(str(exc))}")
            else:
                parsed = loads_json(raw)
                problem = _error_object(parsed)
                usable = (status == 200 and problem is None and isinstance(parsed, dict)
                          and isinstance(parsed.get("choices"), list) and parsed["choices"])
                if usable:
                    with self._lock:
                        self._failures = 0
                    return parsed, time.monotonic() - started
                code = status
                if status == 200:  # the failure is reported in the body, or the body is unusable
                    code = _error_code(problem) or 502
                if status != 200 or problem is not None:
                    text = _failure_text(code, problem, raw)
                else:
                    text = ("HTTP 200 but the response had no usable choices: "
                            + re.sub(r"\s+", " ", raw.decode("utf-8", "replace")).strip()[:200])
                text = self._scrub(text)
                if code in _ACCOUNT_FATAL:
                    raise ProviderError(text, stop_all=True)
                if code in _MODEL_FATAL:
                    raise ProviderError(text, fatal=True)
                error = ProviderError(text)
                if code not in _RETRY_STATUSES:
                    break  # e.g. 403 (moderation) or a 3xx: this request will not work, others might
                retry_after = _retry_after({k.lower(): v for k, v in resp_headers.items()})
            if attempt < self.max_retries:
                self._sleep(self._delay(attempt, retry_after))
        assert error is not None
        with self._lock:
            self._failures += 1
            tripped = self._failures >= self.max_consecutive_failures
        if tripped:
            raise ProviderError(f"{self._failures} requests in a row failed even after retries (the API or the "
                                f"model seems to be down); the last error: {error}", fatal=True)
        raise error

    def complete(self, system: str, messages: list, meta: dict) -> Reply:
        self.ensure_ready()
        parsed, latency = self._post(self._request_body(system, messages))
        choice = parsed["choices"][0]
        message = choice.get("message") if isinstance(choice, dict) else None
        message = message if isinstance(message, dict) else {}
        usage = usage_from_openrouter(parsed.get("usage"))
        self._add_cost(usage)
        finish = choice.get("finish_reason") if isinstance(choice, dict) else None
        return Reply(text=_text_of(message.get("content")), usage=usage, stop_reason=_STOP_REASONS.get(finish, finish),
                     latency_s=latency, request_id=parsed.get("id"), model=parsed.get("model"),
                     upstream=_upstream_provider(parsed))

    def _add_cost(self, usage: Usage) -> None:
        if usage.cost is not None:
            with self._lock:
                self._spent = (self._spent or 0.0) + usage.cost

    # ---- code tokens

    def _count_raw(self, text: str) -> int:
        body = {"model": self.model, "max_tokens": COUNT_MAX_TOKENS, "messages": [{"role": "user", "content": text}]}
        if "provider" in self.extra:  # same routing as the real requests (the tokenizer is the model's, but be safe)
            body["provider"] = self.extra["provider"]
        parsed, _ = self._post(body)
        usage = usage_from_openrouter(parsed.get("usage"))
        self._add_cost(usage)
        if usage.input_tokens is None:
            raise ProviderError("the response to a token-count request had no usage.prompt_tokens")
        return usage.input_tokens

    def count_tokens(self, text: str) -> Optional[int]:
        """Tokens of `text` alone in this model's tokenizer, or None if they cannot be measured."""
        if not self._count_enabled or not text.strip():
            return None
        with self._lock:
            hit = self._count_cache.get(text)
        if hit is not None:
            return hit
        try:
            self.ensure_ready()
            with self._lock:
                baseline = self._count_baseline
            if baseline is None:
                baseline = self._count_raw("x") - 1  # "x" is one token; the rest is the per-request overhead
                with self._lock:
                    self._count_baseline = baseline
            value = self._count_raw(text) - baseline
        except ProviderError as exc:
            with self._lock:
                self._count_failures += 1
                if exc.fatal or self._count_failures >= 3:
                    self._count_enabled = False
            if not self._count_enabled:
                print(f"warning: code-token counting disabled for {self.model}: {exc}", file=sys.stderr)
            return None
        if value < 1 or value > 4 * len(text) + 8:  # not a sensible count (the request was not counted as asked)
            return None
        with self._lock:
            self._count_failures = 0
            self._count_cache[text] = value
        return value


# ------------------------------------------------------------------------ registry

PROVIDERS = {"mock": MockProvider, "anthropic": AnthropicProvider, "openrouter": OpenRouterProvider}


def make_provider(name: str, model: Optional[str] = None, **options) -> Provider:
    try:
        cls = PROVIDERS[name]
    except KeyError:
        raise ValueError(f"unknown provider {name!r}; available: {', '.join(sorted(PROVIDERS))}") from None
    return cls(model, **options)
