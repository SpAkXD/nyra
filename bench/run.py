#!/usr/bin/env python3
"""Nyra benchmark runner: how often does a model write correct code on the first try?

    python bench/run.py --provider mock                          # pipeline self-test, no API key
    python bench/run.py --provider anthropic --model claude-opus-5-5
    python bench/run.py --provider openrouter --models anthropic/claude-opus-5.5,openai/gpt-6-sol

Every task is given to each model once per language (Nyra, Python, TypeScript and Rust by
default) with the same prompt and the same repair budget. The program the model writes is
run and its standard output is compared with the expected output. See bench/README.md for
the methodology, the metrics and the limitations.

Standard library only; provider SDKs are imported lazily by bench/providers.py.
"""

from __future__ import annotations

import argparse
import concurrent.futures as cf
import contextlib
import dataclasses
import datetime as dt
import fnmatch
import hashlib
import json
import os
import platform
import re
import shlex
import shutil
import statistics
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path
from typing import Callable, Optional

BENCH_DIR = Path(__file__).resolve().parent
REPO_DIR = BENCH_DIR.parent
if str(BENCH_DIR) not in sys.path:
    sys.path.insert(0, str(BENCH_DIR))
if __name__ == "__main__":
    # bench/edit_arms.py and bench/safety.py say `import run`: give them this module, not a second copy of it
    sys.modules.setdefault("run", sys.modules[__name__])

import models as modelsmod  # noqa: E402  (sibling modules)
import providers  # noqa: E402
import report  # noqa: E402
import typecheck  # noqa: E402

TASKS_DIR = BENCH_DIR / "tasks"
SOLUTIONS_DIR = BENCH_DIR / "solutions"
# Task tiers. v1: the original input-free tasks (bench/tasks/*.json). The others live in their own folder and have
# their own reference solutions under bench/solutions/<tier>/ (see "Tiers" below).
TIERS = ("v1", "v2", "edit", "safety")
TIER_DIRS = {"v1": TASKS_DIR, "v2": TASKS_DIR / "v2", "edit": TASKS_DIR / "edit", "safety": TASKS_DIR / "safety"}
RESULTS_DIR = BENCH_DIR / "results"
DEFAULT_SPEC = REPO_DIR / "docs" / "SPEC.md"

SCHEMA_VERSION = 3  # 3: runtime of every passing program (result.runtime_ms, result.timing, run.timing)
LANG_ORDER = report.LANG_ORDER  # the languages, in table-column order; the first is the baseline of comparisons
LANG_ALIASES = {"ts": "typescript", "rs": "rust", "py": "python"}
# What `--langs` defaults to, by tier. The edit tier compares ways of editing, not languages (see bench/edit_arms.py); the
# v2 tier has Python and Nyra references for every task (the mock provider and verify.py need them), the others are
# optional, so a default run leaves Rust out.
DEFAULT_LANGS = {"v1": LANG_ORDER, "v2": ("nyra", "python", "typescript"), "safety": ("nyra", "python"),
                 "edit": ("nyra-edit", "python-rewrite", "python-diff")}
MAX_OUTPUT_BYTES = 1_000_000  # a program that prints more than this is killed (runaway loop)
CHECK_TIMEOUT = 60  # seconds for `nyra check`
BUILD_TIMEOUT = 120  # seconds for `nyra build` and for `rustc` (both include the C compiler / linker)
STORED_STDOUT_CHARS = 4000  # what is kept of a program's output in the result file
STORED_STDERR_CHARS = 2000
NODE_MIN_VERSION = (22, 6)  # the first Node.js that can run TypeScript at all (type stripping behind a flag)
# `node main.ts`: --experimental-transform-types also runs enums, namespaces and constructor parameter properties
# (plain stripping rejects them), and implies stripping, so every TypeScript the model might write is accepted.
# The flags exist from Node 22.6 on; the warning flag keeps the "experimental" notice out of error feedback.
NODE_TS_FLAGS = ("--experimental-transform-types", "--disable-warning=ExperimentalWarning")
# There is no Cargo project, so everything is on the command line: optimized (-O), edition 2021 (plain rustc would
# use 2015), and warnings silenced so that the feedback after a failed build contains the errors only.
RUSTC_FLAGS = ("-O", "--edition", "2021", "-A", "warnings", "--color", "never")
DEFAULT_TIME_RUNS = 3  # timed runs of every passing program (after the run that judged it); the median is kept
STARTUP_RUNS = 5  # runs of the hello-world program that measure a language's start-up cost
PREFLIGHT_ID = "preflight"  # the task id of toolchain self-tests, which are never timed


class HarnessError(Exception):
    """The environment is broken (missing compiler, cannot spawn a process). Not the model's fault."""


class UsageError(Exception):
    """Bad command line."""


# ----------------------------------------------------------------------------- tasks


def parse_version(text: str) -> tuple:
    m = re.match(r"\s*v?(\d+)\.(\d+)", str(text))
    if not m:
        raise ValueError(f"not a version like 0.1: {text!r}")
    return int(m.group(1)), int(m.group(2))


@dataclasses.dataclass(frozen=True)
class Case:
    """One input of a task that reads standard input, and the output it must produce. The `visible` case is the
    example printed in the prompt; the others are hidden from the model. A program passes the task only if it is
    right on every case."""
    stdin: str
    expected_output: str
    visible: bool = False
    name: str = ""


@dataclasses.dataclass(frozen=True)
class Task:
    id: str
    title: str
    prompt: str
    expected_output: str
    min_version: str
    category: str
    difficulty: str
    path: Path
    # Tiers (bench/README.md, "Tiers"). A v1 task has no cases and no stdin. A task with `cases` reads its input from
    # stdin and is judged on all of them; `expected_output` is then the visible example's output.
    tier: str = "v1"
    cases: tuple = ()
    raw: Optional[dict] = dataclasses.field(default=None, compare=False, hash=False, repr=False)  # the JSON as loaded

    @property
    def version(self) -> tuple:
        return parse_version(self.min_version)

    @property
    def example(self) -> Optional[Case]:
        return next((c for c in self.cases if c.visible), None)

    @property
    def hidden_cases(self) -> list:
        return [c for c in self.cases if not c.visible]


_TASK_FIELDS = ("id", "title", "prompt", "expected_output", "min_version", "category")


def parse_cases(data: dict, filename: str, require_expected: bool) -> tuple:
    """The `cases` of a task file: [{"name", "visible", "stdin", "expected_output"}]. Exactly one case is visible (the
    example in the prompt) and there are at least two hidden ones, so that a program that prints a fixed answer
    cannot pass."""
    raw = data["cases"]
    if not isinstance(raw, list) or not raw:
        raise ValueError(f"{filename}: `cases` must be a non-empty list")
    cases = []
    for i, c in enumerate(raw):
        if not isinstance(c, dict) or not isinstance(c.get("stdin"), str):
            raise ValueError(f"{filename}: case {i} needs a string `stdin`")
        expected = c.get("expected_output", "")
        if not isinstance(expected, str):
            raise ValueError(f"{filename}: case {i}: `expected_output` must be a string")
        if require_expected and not expected.strip():
            raise ValueError(f"{filename}: case {i} has an empty expected_output (run: python bench/verify.py --write)")
        cases.append(Case(stdin=c["stdin"], expected_output=expected, visible=bool(c.get("visible", False)),
                          name=str(c.get("name") or ("example" if c.get("visible") else f"hidden{i}"))))
    if sum(c.visible for c in cases) != 1:
        raise ValueError(f"{filename}: exactly one case must be `visible` (the example shown in the prompt)")
    if sum(not c.visible for c in cases) < 2:
        raise ValueError(f"{filename}: at least two hidden cases are needed")
    if len({c.name for c in cases}) != len(cases):
        raise ValueError(f"{filename}: case names must be unique")
    return tuple(cases)


def load_tasks(tasks_dir: Path = TASKS_DIR, require_expected: bool = True, tier: str = "v1") -> list:
    """Load <tasks_dir>/*.json, ordered by (min_version, id). Raises ValueError on a malformed task."""
    tasks, seen = [], set()
    for path in sorted(Path(tasks_dir).glob("*.json")):
        try:
            data = json.loads(path.read_text(encoding="utf-8"))
        except ValueError as exc:
            raise ValueError(f"{path.name}: invalid JSON: {exc}") from None
        has_cases = "cases" in data
        missing = [f for f in _TASK_FIELDS
                   if f not in data and not ((has_cases or tier == "safety") and f == "expected_output")]
        if missing:
            raise ValueError(f"{path.name}: missing field(s): {', '.join(missing)}")
        if data["id"] != path.stem:
            raise ValueError(f"{path.name}: id {data['id']!r} must equal the file name")
        if data["id"] in seen:
            raise ValueError(f"duplicate task id {data['id']!r}")
        seen.add(data["id"])
        parse_version(data["min_version"])
        if not str(data["prompt"]).strip():
            raise ValueError(f"{path.name}: empty prompt")
        cases = parse_cases(data, path.name, require_expected) if has_cases else ()
        expected = next((c.expected_output for c in cases if c.visible), "") if has_cases else data.get("expected_output", "")
        if tier == "v1" and has_cases:
            raise ValueError(f"{path.name}: `cases` belong to the v2 tier (bench/tasks/v2/)")
        if require_expected and not str(expected).strip():
            raise ValueError(f"{path.name}: empty expected_output (run: python bench/verify.py --write)")
        tasks.append(Task(id=data["id"], title=data["title"], prompt=data["prompt"], expected_output=expected,
                          min_version=str(data["min_version"]), category=data["category"],
                          difficulty=data.get("difficulty", ""), path=path, tier=data.get("tier", tier), cases=cases,
                          raw=data))
    tasks.sort(key=lambda t: (t.version, t.id))
    return tasks


def load_tier(tier: str, require_expected: bool = True) -> list:
    """The tasks of one tier: v1 (the original input-free tasks), v2 (stdin, hidden inputs), edit, safety."""
    if tier not in TIERS:
        raise UsageError(f"--tier must be one of {', '.join(TIERS)}")
    if tier == "v1":
        return load_tasks(TIER_DIRS["v1"], require_expected)
    tasks = (load_tasks(TIER_DIRS[tier], require_expected and tier != "safety", tier=tier)
             if TIER_DIRS[tier].is_dir() else [])
    if tier in ("v2", "edit"):  # the three ids spaces must not overlap: result files and references are keyed by id
        clash = {t.id for t in tasks} & {t.id for t in load_tasks(TIER_DIRS["v1"], False)}
        if clash:
            raise ValueError(f"task id(s) of the {tier} tier also exist in v1: {', '.join(sorted(clash))}")
    return tasks


def tasks_digest(tasks_dir: Path = TASKS_DIR, tier: str = "v1") -> str:
    """Hash of the task files. v1: the JSON files only (so the hash of the original set never changes); other
    tiers also hash the files next to the JSON (the programs an edit task starts from)."""
    h = hashlib.sha256()
    paths = (sorted(Path(tasks_dir).glob("*.json")) if tier == "v1"
             else sorted(p for p in Path(tasks_dir).iterdir() if p.is_file()))
    for path in paths:
        h.update(path.name.encode())
        h.update(path.read_bytes().replace(b"\r\n", b"\n"))
    return h.hexdigest()


def task_prompt(task: Task) -> str:
    """What the model is shown as the task: the prompt, and for a task with hidden inputs the one visible example.
    Everything else about the hidden cases stays out of the prompt (and out of the repair feedback)."""
    ex = task.example
    if ex is None:
        return task.prompt

    def block(text: str) -> str:
        return text if text.endswith("\n") or not text else text + "\n"

    return (task.prompt.rstrip() + "\n\nThe program will be run on several inputs; you see only this one example.\n\n"
            f"<example_input>\n{block(ex.stdin)}</example_input>\n\n"
            f"<example_output>\n{block(ex.expected_output)}</example_output>")


# ----------------------------------------------------------- prompts and feedback text
#
# Everything the model is ever told is built here, so it can be audited in one place.
# The system prompts of all languages are deliberately parallel: a first sentence that names the
# language and how its programs are run, then the same task paragraph and the same reply rule.
# Only Nyra's carries a spec, because the model cannot know the language.

_REPLY_RULE = ("Reply with exactly one fenced code block that contains the complete program, "
               "and no other text.")


def _task_paragraph(language: str) -> str:
    return (f"Solve the task you are given with a complete {language} program. The program takes no input, and "
            "only what it prints to standard output is checked, so it must print exactly what the task describes.")


def _task_paragraph_stdin(language: str) -> str:
    """The task paragraph for the v2 tier: the program reads standard input, and is judged on inputs the model does
    not see."""
    return (f"Solve the task you are given with a complete {language} program. The program reads its input from "
            "standard input and prints its result to standard output. It is run on several inputs, of which you see "
            "only one example; for each input its standard output must be exactly what the task describes, so the "
            "program has to work for every valid input and not only for the example.")


_NYRA_SYSTEM = """\
You write programs in Nyra, a new programming language that you have not seen before. \
The complete language specification is below. It is the only documentation you have.

<nyra_spec>
{spec}
</nyra_spec>

""" + _task_paragraph("Nyra") + "\n\n" + _REPLY_RULE

_PYTHON_SYSTEM = ("You write programs in Python 3, using only the standard library.\n\n"
                  + _task_paragraph("Python") + "\n\n" + _REPLY_RULE)

_TYPESCRIPT_SYSTEM = ("You write programs in TypeScript. The program is run directly with Node.js, which removes "
                      "the type annotations without checking them, and it may use only what Node.js itself "
                      "provides (no npm packages).\n\n" + _task_paragraph("TypeScript") + "\n\n" + _REPLY_RULE)

_RUST_SYSTEM = ("You write programs in Rust (2021 edition), using only the standard library. The program is a "
                "single file with a `main` function, compiled with `rustc` (no Cargo, no external crates).\n\n"
                + _task_paragraph("Rust") + "\n\n" + _REPLY_RULE)

_FIX = ("Fix the program. Reply with exactly one fenced code block that contains the complete "
        "corrected program, and no other text.")


def clip_head(text: str, max_lines: int = 40, max_chars: int = 3000) -> str:
    lines = text.rstrip("\n").split("\n")
    kept = lines[:max_lines]
    out = "\n".join(line if len(line) <= 200 else line[:200] + "..." for line in kept)
    if len(out) > max_chars:
        out = out[:max_chars] + "..."
    elif len(lines) > max_lines:
        out += f"\n... ({len(lines) - max_lines} more lines)"
    return out


def clip_tail(text: str, max_lines: int = 30, max_chars: int = 3000) -> str:
    lines = text.rstrip("\n").split("\n")
    out = "\n".join(lines[-max_lines:])
    if len(lines) > max_lines:
        out = f"... ({len(lines) - max_lines} earlier lines)\n" + out
    if len(out) > max_chars:
        out = "..." + out[-max_chars:]
    return out


def fb_no_code() -> str:
    return ("Your reply did not contain a fenced code block, so there was no program to run. " + _REPLY_RULE)


def fb_compile(tool: str, output: str) -> str:
    return f"{tool} rejected your program. Its output:\n\n<compiler_output>\n{clip_head(output, 60, 6000)}\n</compiler_output>\n\n{_FIX}"


def fb_toolchain(stderr: str) -> str:
    return ("The compiler accepted your program, but building it failed afterwards. That is a compiler bug, "
            "not necessarily a mistake in your program. Details:\n\n"
            f"<build_output>\n{clip_tail(stderr, 15, 1500)}\n</build_output>\n\n"
            "If you can, rewrite the program so that it avoids the construct that triggers the failure. " + _REPLY_RULE)


def fb_runtime(exit_code, stderr: str, stdout: str) -> str:
    parts = [f"Your program crashed (exit code {exit_code})."]
    if stderr.strip():
        parts.append(f"<stderr>\n{clip_tail(stderr)}\n</stderr>")
    if stdout.strip():
        parts.append(f"It had printed this before stopping:\n<program_output>\n{clip_head(stdout, 20, 1500)}\n</program_output>")
    parts.append(_FIX)
    return "\n\n".join(parts)


def fb_timeout(timeout: float, stdout: str) -> str:
    parts = [f"Your program did not finish within {timeout:g} seconds and was stopped "
             "(probably an endless loop, or far too slow)."]
    if stdout.strip():
        parts.append(f"It had printed this so far:\n<program_output>\n{clip_head(stdout, 20, 1500)}\n</program_output>")
    parts.append(_FIX)
    return "\n\n".join(parts)


def fb_output_limit(stdout: str) -> str:
    return (f"Your program printed more than {MAX_OUTPUT_BYTES} bytes and was stopped (probably an endless loop). "
            f"The start of its output:\n<program_output>\n{clip_head(stdout, 20, 1500)}\n</program_output>\n\n{_FIX}")


def fb_wrong(actual: str, expected: str) -> str:
    """Wrong-output feedback. It never shows the expected output (that would let a model copy the
    answer), only where the first difference is and how many lines were expected."""
    exp_lines = expected.split("\n") if expected else []
    act_lines = actual.split("\n") if actual else []
    head = ("Your program ran without errors, but its output does not match what the task asks for. "
            f"The first difference is on line {first_diff_line(expected, actual)}. "
            f"The expected output has {len(exp_lines)} line(s); your program printed {len(act_lines)}.")
    shown = (f"Your program printed:\n<program_output>\n{clip_head(actual)}\n</program_output>"
             if actual else "Your program printed nothing.")
    return f"{head}\n\n{shown}\n\nRe-read the task. {_FIX}"


def fb_hidden(result: "EvalResult", expected: str, index: int, total: int, timeout: float) -> str:
    """Feedback for a program that is right on the example but fails a hidden input. It never shows the hidden input
    or the expected output (that would let the model copy them): only that it failed, how, and where the first line
    that differs is."""
    where = f"hidden input {index} of {total} (you cannot see it)"
    advice = ("Think about the cases the task describes that the example does not show (empty input, ties, bad "
              "lines, limits). Do not special-case the example. " + _FIX)
    if result.kind == "wrong_output":
        act = normalize_output(result.stdout)
        return (f"Your program printed the right output for the example, but its output is wrong for {where}. "
                f"The first difference is on line {first_diff_line(normalize_output(expected), act)} of the output; "
                f"your program printed {len(act.split(chr(10))) if act else 0} line(s).\n\n{advice}")
    if result.kind == "runtime_error":
        return (f"Your program printed the right output for the example, but it crashed on {where} "
                f"(exit code {result.exit_code}).\n\n<stderr>\n{clip_tail(result.stderr)}\n</stderr>\n\n{advice}")
    if result.kind == "timeout":
        return (f"Your program printed the right output for the example, but on {where} it did not finish within "
                f"{timeout:g} seconds and was stopped.\n\n{advice}")
    if result.kind == "output_limit":
        return (f"Your program printed the right output for the example, but on {where} it printed more than "
                f"{MAX_OUTPUT_BYTES} bytes and was stopped.\n\n{advice}")
    return result.feedback


# --------------------------------------------------------------- code and output handling

_FENCE_RE = re.compile(r"^[ \t]*(?P<fence>`{3,}|~{3,})[^\n]*\n(?P<body>.*?)^[ \t]*(?P=fence)[ \t]*$", re.S | re.M)


def extract_code(reply: str) -> Optional[str]:
    """The body of the first fenced code block of a reply, or None (no block, unterminated, empty)."""
    m = _FENCE_RE.search(reply.replace("\r\n", "\n"))
    if not m:
        return None
    code = m.group("body").rstrip("\n")
    return code if code.strip() else None


_FENCE_TAGGED_RE = re.compile(r"^[ \t]*(?P<fence>`{3,}|~{3,})[ \t]*(?P<info>[^\n]*)\n(?P<body>.*?)^[ \t]*(?P=fence)[ \t]*$",
                              re.S | re.M)


def extract_code_tagged(reply: str, tags: tuple) -> Optional[str]:
    """The program of a reply, for the tiers after v1: the LAST fenced block whose language tag is one of `tags`
    (```python, ```nyra, ...); if no block has such a tag, the last block. A model that thinks aloud often quotes the
    example's output in a plain block before it gives the program (the example is in the prompt of these tiers), and
    the program comes last. The same rule for every language; v1 keeps "the first block" (extract_code)."""
    blocks = []
    for m in _FENCE_TAGGED_RE.finditer(reply.replace("\r\n", "\n")):
        body = m.group("body").rstrip("\n")
        if body.strip():
            tag = (m.group("info").split() or [""])[0].lower().strip("{}.")
            blocks.append((tag, body))
    if not blocks:
        return None
    tagged = [body for tag, body in blocks if tag in tags]
    return (tagged or [body for _, body in blocks])[-1]


def normalize_output(text: str) -> str:
    """CRLF -> LF, trailing whitespace stripped from every line, trailing blank lines dropped.
    Leading whitespace and leading blank lines are significant (patterns depend on them)."""
    lines = [line.rstrip() for line in text.replace("\r\n", "\n").replace("\r", "\n").split("\n")]
    while lines and lines[-1] == "":
        lines.pop()
    return "\n".join(lines)


def first_diff_line(expected: str, actual: str) -> int:
    """1-based number of the first line where two normalized outputs differ."""
    e, a = expected.split("\n"), actual.split("\n")
    for i, (x, y) in enumerate(zip(e, a), 1):
        if x != y:
            return i
    return min(len(e), len(a)) + 1


def code_size(code: str) -> tuple:
    """(characters, non-blank lines) of a program, ignoring leading/trailing whitespace."""
    stripped = code.strip()
    return len(stripped), sum(1 for line in stripped.splitlines() if line.strip())


# --------------------------------------------------------------------- process running


@dataclasses.dataclass
class Proc:
    returncode: Optional[int]
    stdout: bytes
    stderr: bytes
    timed_out: bool = False
    truncated: bool = False
    elapsed: float = 0.0
    spawn_error: Optional[str] = None


def run_limited(argv, *, cwd, env, timeout: float, max_output: int = MAX_OUTPUT_BYTES,
                stdin: Optional[bytes] = None) -> Proc:
    """Run a command with a wall-clock timeout and a cap on captured output.

    Output is read on the fly and the process is killed once it exceeds `max_output`, so a
    program stuck in `while true { print(1) }` cannot exhaust memory. The command must be the
    program itself (not a launcher that spawns it): killing only reaches the direct child.
    `stdin` is what the program reads from standard input (default: nothing, an empty stdin).
    """
    start = time.perf_counter()
    proc = None
    for attempt in range(4):
        try:
            proc = subprocess.Popen([str(a) for a in argv], cwd=str(cwd), env=env,
                                    stdin=subprocess.DEVNULL if stdin is None else subprocess.PIPE,
                                    stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            break
        except (FileNotFoundError, NotADirectoryError) as exc:  # not transient
            return Proc(None, b"", b"", spawn_error=f"{type(exc).__name__}: {exc}")
        except OSError as exc:
            # Windows can refuse to start a file that was written a moment ago (antivirus scan): retry briefly.
            if attempt == 3:
                return Proc(None, b"", b"", spawn_error=f"{type(exc).__name__}: {exc}")
            time.sleep(0.25 * 2 ** attempt)
    assert proc is not None

    bufs = {"out": bytearray(), "err": bytearray()}
    truncated = threading.Event()

    def pump(stream, key):
        buf = bufs[key]
        try:
            while True:
                chunk = stream.read1(65536)
                if not chunk:
                    return
                room = max_output - len(buf)
                if room > 0:
                    buf += chunk[:room]
                if len(chunk) > room:
                    truncated.set()
                    with contextlib.suppress(OSError):
                        proc.kill()
        except (OSError, ValueError):
            return

    def feed():
        try:
            proc.stdin.write(stdin)
        except (OSError, ValueError):  # the program exited without reading all of its input
            pass
        finally:
            with contextlib.suppress(OSError, ValueError):
                proc.stdin.close()

    threads = [threading.Thread(target=pump, args=(proc.stdout, "out"), daemon=True),
               threading.Thread(target=pump, args=(proc.stderr, "err"), daemon=True)]
    if stdin is not None:
        threads.append(threading.Thread(target=feed, daemon=True))
    for t in threads:
        t.start()
    timed_out = False
    try:
        proc.wait(timeout=timeout)
    except subprocess.TimeoutExpired:
        timed_out = True
        with contextlib.suppress(OSError):
            proc.kill()
        proc.wait()
    elapsed = time.perf_counter() - start  # until the process exited: draining the pipes is not its time
    for t, stream in zip(threads, (proc.stdout, proc.stderr)):
        t.join(timeout=5)
        if not t.is_alive():  # never close a pipe another thread is still blocked on
            with contextlib.suppress(OSError, ValueError):
                stream.close()
    return Proc(proc.returncode, bytes(bufs["out"]), bytes(bufs["err"]), timed_out=timed_out,
                truncated=truncated.is_set(), elapsed=elapsed)


_SECRET_HINTS = ("KEY", "TOKEN", "SECRET", "PASSWORD", "PASSWD", "CREDENTIAL")


def child_env(workdir: Path) -> dict:
    """Environment for programs and compilers: secrets removed, temp files confined to `workdir`.

    The Nyra compiler writes its intermediate C file to <temp dir>/nyra/<name>.c, so a private
    temp dir per evaluation keeps parallel jobs from overwriting each other's files.
    """
    env = {k: v for k, v in os.environ.items() if not any(h in k.upper() for h in _SECRET_HINTS)}
    for var in ("TMP", "TEMP", "TMPDIR"):
        env[var] = str(workdir)
    env["PYTHONIOENCODING"] = "utf-8"
    return env


def _rmtree(path: Path) -> None:
    for _ in range(5):  # Windows may briefly lock a freshly built exe (antivirus scan)
        try:
            shutil.rmtree(path)
            return
        except OSError:
            time.sleep(0.2)
    shutil.rmtree(path, ignore_errors=True)


@contextlib.contextmanager
def scratch_dir():
    path = Path(tempfile.mkdtemp(prefix="nyra-bench-"))
    try:
        yield path
    finally:
        _rmtree(path)


def write_source(path: Path, code: str) -> None:
    text = code.replace("\r\n", "\n").replace("\r", "\n")
    path.write_bytes((text.rstrip("\n") + "\n").encode("utf-8"))


def scrub_paths(text: str, workdir: Path) -> str:
    """Remove the private scratch directory from tool output before the model sees it
    (a Python traceback says `File "main.py"`, not `File "C:\\Users\\...\\Temp\\nyra-bench-x\\main.py"`)."""
    variants = {str(workdir), workdir.as_posix(), str(workdir.resolve()), workdir.resolve().as_posix()}
    for variant in sorted(variants, key=len, reverse=True):
        for prefix in ("file:///", "file://", ""):  # Node prints module URLs: file:///C:/Users/.../main.ts
            for sep in ("\\", "/"):
                text = text.replace(prefix + variant + sep, "")
        text = text.replace(variant, ".")
    return text


# ------------------------------------------------------------------------- evaluation


@dataclasses.dataclass
class EvalResult:
    passed: bool
    kind: str  # pass | no_code | compile_error | toolchain_error | runtime_error | timeout | output_limit | wrong_output
    feedback: str = ""  # what the model is told if it gets a repair attempt
    errors: list = dataclasses.field(default_factory=list)  # Nyra compiler diagnostics (parsed JSON)
    stdout: str = ""
    stderr: str = ""
    exit_code: Optional[int] = None
    compile_ms: Optional[float] = None
    run_ms: Optional[float] = None  # wall clock of the run that was judged (start-up included)
    # Only for a passing program that was timed: the median of the timed runs minus the language's start-up cost
    # (see Language.time_result), and the raw numbers behind it.
    runtime_ms: Optional[float] = None
    timing: Optional[dict] = None
    # Only for a task with hidden inputs: one {"name", "visible", "passed", "kind"} per case that was run (the cases
    # after the first failure are not run). `passed` is True only if every case passed.
    cases: Optional[list] = None

    def to_dict(self) -> dict:
        out = {
            "passed": self.passed, "kind": self.kind, "errors": self.errors,
            "stdout": self.stdout[:STORED_STDOUT_CHARS], "stderr": self.stderr[-STORED_STDERR_CHARS:],
            "exit_code": self.exit_code,
            "compile_ms": None if self.compile_ms is None else round(self.compile_ms, 1),
            "run_ms": None if self.run_ms is None else round(self.run_ms, 1),
            "runtime_ms": None if self.runtime_ms is None else round(self.runtime_ms, 2),
            "timing": self.timing,
        }
        if self.cases is not None:
            out["cases"] = self.cases
        return out


_PY_SYNTAX_ERRORS = ("SyntaxError", "IndentationError", "TabError")


def _is_python_syntax_error(stderr: str) -> bool:
    last = next((ln for ln in reversed(stderr.strip().splitlines()) if ln.strip()), "")
    return last.split(":")[0].strip().rsplit(".", 1)[-1] in _PY_SYNTAX_ERRORS


def _python_diagnose(stderr: str, stdout: str) -> Optional[str]:
    """Name of the tool that rejected the program before running it, or None if it crashed while running."""
    return "Python" if _is_python_syntax_error(stderr) else None


# Node prints an uncaught error as: file:line, the source line and a caret, a blank line, `SyntaxError [CODE]: text`
# (or `TypeError: text` ...), then the stack. A program that was rejected before it started has no frame of its
# own file in that stack; one that crashed while running does.
_NODE_ERROR_LINE = re.compile(r"^([A-Za-z]*Error)(?: \[[A-Za-z0-9_]+\])?: ", re.M)
_NODE_USER_FRAME = re.compile(r"^\s+at .*\bmain\.ts:\d+", re.M)
_NODE_INTERNAL_FRAME = re.compile(r"^\s+at (?:.* \()?(?:node:|internal/)[^\s)]*\)?\s*\{?\s*$")
_NODE_TRAILER = re.compile(r"^Node\.js v\d+")
_NODE_ERROR_CODE_TAIL = re.compile(r"\n\s*code: '[A-Za-z0-9_]+'\n\}")


def clean_node_stderr(text: str) -> str:
    """Drop what is noise to the model: Node's own stack frames, the version line, the `{ code: ... }` tail."""
    kept = [ln for ln in text.replace("\r\n", "\n").split("\n")
            if not _NODE_INTERNAL_FRAME.match(ln) and not _NODE_TRAILER.match(ln)]
    return _NODE_ERROR_CODE_TAIL.sub("", "\n".join(kept)).strip("\n")


def _is_node_syntax_error(stderr: str) -> bool:
    m = _NODE_ERROR_LINE.search(stderr)
    return bool(m) and m.group(1) == "SyntaxError" and not _NODE_USER_FRAME.search(stderr)


def _node_diagnose(stderr: str, stdout: str) -> Optional[str]:
    return "Node.js" if _is_node_syntax_error(stderr) else None


def judge_run(proc: Proc, task: Task, timeout: float, *, workdir: Path, compile_ms: Optional[float] = None,
              diagnose=None, clean_stderr=None) -> EvalResult:
    """Turn a finished process into a verdict. Shared by all languages, so the rules are identical.

    `diagnose(stderr, stdout)` names the tool when a non-zero exit means "rejected before it ran" (a syntax
    error in an interpreted language); `clean_stderr` removes tool noise from stderr before the model sees it.
    """
    if proc.spawn_error:
        raise HarnessError(f"cannot start the program: {proc.spawn_error}")
    stdout = proc.stdout.decode("utf-8", "replace")
    stderr = scrub_paths(proc.stderr.decode("utf-8", "replace"), workdir)
    if clean_stderr is not None:
        stderr = clean_stderr(stderr)
    base = dict(stdout=stdout, stderr=stderr, exit_code=proc.returncode, compile_ms=compile_ms,
                run_ms=proc.elapsed * 1000)
    if proc.timed_out:
        return EvalResult(False, "timeout", feedback=fb_timeout(timeout, stdout), **base)
    if proc.truncated:
        return EvalResult(False, "output_limit", feedback=fb_output_limit(stdout), **base)
    if proc.returncode != 0:
        tool = diagnose(stderr, stdout) if diagnose is not None else None
        if tool:
            return EvalResult(False, "compile_error", feedback=fb_compile(tool, clip_tail(stderr)), **base)
        return EvalResult(False, "runtime_error", feedback=fb_runtime(proc.returncode, stderr, stdout), **base)
    expected, actual = normalize_output(task.expected_output), normalize_output(stdout)
    if actual == expected:
        return EvalResult(True, "pass", **base)
    return EvalResult(False, "wrong_output", feedback=fb_wrong(actual, expected), **base)


@dataclasses.dataclass
class Built:
    """A program that is ready to run in its scratch directory: the command, and what building it took."""
    argv: list
    compile_ms: Optional[float] = None


# Timed runs never overlap each other (parallel jobs would slow each other down); compilers of other jobs may
# still run meanwhile, which is why bench/speed.py, which runs nothing else, gives the cleaner numbers.
TIMING_LOCK = threading.Lock()


def time_command(argv, *, cwd, env, runs: int, timeout: float, expected: Optional[str] = None) -> tuple:
    """Run a built program `runs` times, one after the other. Returns (wall-clock milliseconds of each run, problem):
    problem is None, or why a run did not count (it failed, or printed something else than `expected`)."""
    times: list = []
    with TIMING_LOCK:
        for _ in range(runs):
            proc = run_limited(argv, cwd=cwd, env=env, timeout=timeout)
            if proc.spawn_error or proc.timed_out or proc.truncated or proc.returncode != 0:
                why = proc.spawn_error or ("timeout" if proc.timed_out else "output limit" if proc.truncated
                                           else f"exit code {proc.returncode}")
                return times, f"a timed run failed ({why})"
            if expected is not None and normalize_output(proc.stdout.decode("utf-8", "replace")) != normalize_output(expected):
                return times, "a timed run printed a different output"
            times.append(proc.elapsed * 1000)
    return times, None


class Language:
    name = ""
    display = ""
    ext = ""
    hello_world = ""  # prints 42; the toolchain self-test, and the program whose run time is the start-up cost

    def __init__(self, timeout: float = 10.0, time_runs: int = 0):
        self.timeout = timeout
        self.time_runs = time_runs  # timed runs of every passing program (0: no timing)
        self._startup: Optional[float] = None
        self._startup_done = False
        self._startup_lock = threading.Lock()

    @property
    def system_prompt(self) -> str:
        raise NotImplementedError

    def reference_path(self, task_id: str) -> Path:
        path = SOLUTIONS_DIR / self.name / f"{task_id}{self.ext}"
        if not path.is_file():  # later tiers keep their references apart: bench/solutions/<tier>/<language>/
            for tier in ("v2", "safety"):
                tiered = SOLUTIONS_DIR / tier / self.name / f"{task_id}{self.ext}"
                if tiered.is_file():
                    return tiered
        return path

    def reference_code(self, task_id: str) -> Optional[str]:
        path = self.reference_path(task_id)
        return path.read_text(encoding="utf-8") if path.is_file() else None

    def system_prompt_for(self, task: Task) -> str:
        """The system prompt for one task. It differs from `system_prompt` only for a task that reads its input from
        stdin: the paragraph that says "the program takes no input" is replaced by one that explains the hidden inputs."""
        base = self.system_prompt
        if not task.cases:
            return base
        old = _task_paragraph(self.display)
        return base.replace(old, _task_paragraph_stdin(self.display)) if old in base else base + "\n\n" + _task_paragraph_stdin(self.display)

    def prompt_for(self, task: Task) -> str:
        """The user message that gives the model the task (the prompt, plus the example of a stdin task)."""
        return task_prompt(task)

    fence_tags: tuple = ()  # the language tags of the fenced block that holds this language's program

    def extract(self, reply: str, task: Task) -> Optional[str]:
        """The program in a reply: the first fenced block for the original tasks, the last block tagged with this language
        for the later tiers (extract_code_tagged)."""
        return extract_code(reply) if task.tier == "v1" else extract_code_tagged(reply, self.fence_tags)

    def no_code_feedback(self) -> str:
        return fb_no_code()

    def build(self, code: str, wd: Path, env: dict):
        """Write the program into `wd` and compile it if the language needs that. Returns a Built, or the
        EvalResult of a program that never got to run (compile error, toolchain error)."""
        raise NotImplementedError

    def diagnose(self, stderr: str, stdout: str) -> Optional[str]:
        """The tool that rejected the program before it ran (an interpreter's syntax error), or None."""
        return None

    checker: Optional[typecheck.Checker] = None  # --python-typecheck / --ts-typecheck: a real type checker run first

    def run_checker(self, filename: str, wd: Path, env: dict) -> Optional[EvalResult]:
        """Type-check the program that is already written in `wd`. None when it passes (or there is no checker); the
        verdict of a program the checker rejects otherwise. It counts as a compile error, like a Nyra program that
        `nyra check` rejects, and the checker's own messages are the repair feedback."""
        if self.checker is None:
            return None
        if self.checker.shim:
            (wd / typecheck.TS_SHIM_NAME).write_text(self.checker.shim, encoding="utf-8", newline="\n")
        started = time.perf_counter()
        proc = run_limited(self.checker.command(filename), cwd=wd, env=env, timeout=CHECK_TIMEOUT)
        if proc.spawn_error:
            raise HarnessError(f"cannot run the type checker {self.checker.name}: {proc.spawn_error}")
        if proc.timed_out:
            raise HarnessError(f"the type checker {self.checker.name} took longer than {CHECK_TIMEOUT} s")
        if proc.returncode == 0:
            return None
        text = typecheck.clean_output((proc.stdout + proc.stderr).decode("utf-8", "replace"), wd)
        return EvalResult(False, "compile_error", feedback=fb_compile(self.checker.display, text), stderr=text,
                          exit_code=proc.returncode, compile_ms=(time.perf_counter() - started) * 1000)

    def type_note(self) -> str:
        """The sentence that tells the model its program is type-checked (empty without a checker)."""
        return " " + typecheck.system_note(self.name, self.checker) if self.checker is not None else ""

    def clean_stderr(self, stderr: str) -> str:
        return stderr

    def evaluate(self, code: str, task: Task, timed: bool = True) -> EvalResult:
        if task.tier == "safety":  # judged by what the program does with canaries, not by its output (bench/safety.py)
            import safety
            return safety.evaluate(self, code, task)
        with scratch_dir() as wd:
            env = child_env(wd)
            built = self.build(code, wd, env)
            if isinstance(built, EvalResult):
                return built
            return self.run_built(built, task, wd, env, timed)

    def run_built(self, built: Built, task: Task, wd: Path, env: dict, timed: bool = True) -> EvalResult:
        """Run a built program once and judge it; time it as well if it passed and timing is on."""
        if task.cases:
            return self.run_cases(built, task, wd, env)
        proc = run_limited(built.argv, cwd=wd, env=env, timeout=self.timeout)
        result = judge_run(proc, task, self.timeout, workdir=wd, compile_ms=built.compile_ms,
                           diagnose=self.diagnose, clean_stderr=self.clean_stderr)
        if timed and result.passed and self.time_runs > 0 and task.id != PREFLIGHT_ID:
            self.time_result(result, built, task, wd, env)
        return result

    def run_cases(self, built: Built, task: Task, wd: Path, env: dict) -> EvalResult:
        """Run a program on every case of a task with hidden inputs (the visible example first, then the hidden ones,
        in file order) and judge each exactly like a single run. The program passes only if it is right on all of
        them, which is what makes printing a fixed answer useless. The first failing case ends the evaluation; its
        verdict is the verdict of the attempt. Tasks with cases are not timed."""
        results = []
        ordered = sorted(task.cases, key=lambda c: not c.visible)
        hidden_total = len(task.hidden_cases)
        first = None
        for case in ordered:
            single = dataclasses.replace(task, expected_output=case.expected_output, cases=())
            proc = run_limited(built.argv, cwd=wd, env=env, timeout=self.timeout, stdin=case.stdin.encode("utf-8"))
            result = judge_run(proc, single, self.timeout, workdir=wd, compile_ms=built.compile_ms,
                               diagnose=self.diagnose, clean_stderr=self.clean_stderr)
            results.append({"name": case.name, "visible": case.visible, "passed": result.passed, "kind": result.kind})
            if first is None:
                first = result
            if not result.passed:
                if not case.visible:  # the model may not learn the hidden input or its expected output
                    index = [c.name for c in task.hidden_cases].index(case.name) + 1
                    result.feedback = fb_hidden(result, case.expected_output, index, hidden_total, self.timeout)
                result.cases = results
                return result
        assert first is not None
        first.cases = results
        return first

    def time_result(self, result: EvalResult, built: Built, task: Task, wd: Path, env: dict) -> None:
        """Run a program that passed `time_runs` more times and keep the median wall clock, minus the language's
        start-up cost (the median run time of its hello-world program, measured the same way), as runtime_ms.

        The run that judged the program is not one of them: it is a warm-up (the first start of a fresh
        executable can be slower, for example while an antivirus scans it). The compile time is not included."""
        startup = self.startup_ms()
        times, problem = time_command(built.argv, cwd=wd, env=env, runs=self.time_runs, timeout=self.timeout,
                                      expected=task.expected_output)
        result.timing = {"runs_ms": [round(t, 2) for t in times],
                         "startup_ms": None if startup is None else round(startup, 2)}
        if problem:
            result.timing["error"] = problem
            return
        median = statistics.median(times)
        result.timing["median_ms"] = round(median, 2)
        result.runtime_ms = max(0.0, median - (startup or 0.0))

    def startup_ms(self) -> Optional[float]:
        """Median wall clock of the hello-world program: starting the interpreter, the runtime or the process.
        Measured once per Language (None if it could not be measured)."""
        with self._startup_lock:
            if not self._startup_done:
                self._startup_done = True
                hello = Task(PREFLIGHT_ID, "", "", "42\n", "0.1", "", "", Path("."))
                with scratch_dir() as wd:
                    env = child_env(wd)
                    built = self.build(self.hello_world, wd, env)
                    if not isinstance(built, EvalResult):
                        run_limited(built.argv, cwd=wd, env=env, timeout=self.timeout)  # warm-up
                        times, problem = time_command(built.argv, cwd=wd, env=env, runs=max(STARTUP_RUNS, self.time_runs),
                                                      timeout=self.timeout, expected=hello.expected_output)
                        if not problem:
                            self._startup = statistics.median(times)
            return self._startup

    def measured_startup_ms(self) -> Optional[float]:
        """The start-up cost if it has been measured already (never measures it)."""
        return self._startup

    def preflight(self) -> list:
        """Check that the toolchain works before any (paid) request is made. Returns warnings."""
        hello = Task(PREFLIGHT_ID, "", "", "42\n", "0.1", "", "", Path("."))
        result = self.evaluate(self.hello_world, hello)
        if not result.passed:
            raise HarnessError(f"{self.display} toolchain self-test failed ({result.kind}): "
                               f"{(result.stderr or result.stdout).strip()[:500]}")
        return []


class PythonLang(Language):
    name = "python"
    display = "Python"
    ext = ".py"
    fence_tags = ("python", "py", "python3", "python-rewrite")
    hello_world = "print(42)\n"

    def __init__(self, timeout: float = 10.0, time_runs: int = 0, checker: Optional[typecheck.Checker] = None):
        super().__init__(timeout, time_runs)
        self.checker = checker

    @property
    def system_prompt(self) -> str:
        if self.checker is None:
            return _PYTHON_SYSTEM
        return _PYTHON_SYSTEM.replace(_task_paragraph("Python"), _task_paragraph("Python") + self.type_note())

    def diagnose(self, stderr: str, stdout: str) -> Optional[str]:
        return _python_diagnose(stderr, stdout)

    def build(self, code: str, wd: Path, env: dict):
        write_source(wd / "main.py", code)
        rejected = self.run_checker("main.py", wd, env)  # --python-typecheck: mypy or pyright, before the program runs
        if rejected is not None:
            return rejected
        # -I: isolated mode (no user site-packages, no PYTHON* variables); -X utf8: same text encoding everywhere
        return Built([sys.executable, "-I", "-X", "utf8", "main.py"])


class NyraLang(Language):
    name = "nyra"
    display = "Nyra"
    ext = ".nyra"
    fence_tags = ("nyra", "ny", "nyra-edit")
    hello_world = "fn main() {\n    print(42)\n}\n"

    def __init__(self, nyra_bin: Path, backend: str = "native", spec_path: Path = DEFAULT_SPEC, timeout: float = 10.0,
                 node: str = "node", time_runs: int = 0):
        super().__init__(timeout, time_runs)
        if backend not in ("native", "js"):
            raise UsageError("--backend must be native or js")
        self.bin = Path(nyra_bin)
        self.node = node  # runs the JavaScript backend's output
        self.backend = backend
        self.spec_path = Path(spec_path)
        try:
            self.spec = self.spec_path.read_text(encoding="utf-8")
        except OSError as exc:
            raise HarnessError(f"cannot read the Nyra spec {self.spec_path}: {exc}") from None
        self.spec_sha256 = hashlib.sha256(self.spec.replace("\r\n", "\n").encode("utf-8")).hexdigest()
        m = re.search(r"^#\s*Nyra\s+v?(\d+\.\d+)", self.spec, re.M)
        self.spec_version = m.group(1) if m else None
        self._version_text: Optional[str] = None

    @property
    def system_prompt(self) -> str:
        return _NYRA_SYSTEM.format(spec=self.spec.strip())

    def version_text(self) -> str:
        """Output of `nyra --version`, e.g. "nyra 0.1.0" ("" if it cannot be read)."""
        if self._version_text is None:
            p = run_limited([self.bin, "--version"], cwd=REPO_DIR, env=dict(os.environ), timeout=30)
            self._version_text = (p.stdout + p.stderr).decode("utf-8", "replace").strip()
        return self._version_text

    def version(self) -> Optional[tuple]:
        m = re.search(r"(\d+)\.(\d+)(?:\.\d+)?", self.version_text())
        return (int(m.group(1)), int(m.group(2))) if m else None

    def preflight(self) -> list:
        if self.version() is None:
            raise HarnessError(f"{self.bin} does not look like the Nyra compiler (`--version` printed "
                               f"{self.version_text()!r})")
        warnings = super().preflight()
        ver = self.version()
        if self.spec_version and ver and parse_version(self.spec_version) != ver:
            warnings.append(f"{self.spec_path.name} describes Nyra v{self.spec_version} but the compiler is "
                            f"{self.version_text()}: the model is shown a spec that does not match the compiler")
        return warnings

    def build(self, code: str, wd: Path, env: dict):
        js = self.backend == "js"
        write_source(wd / "main.nyra", code)
        started = time.perf_counter()
        # The file name is relative so diagnostics read `"file":"main.nyra"` (no temp paths in the prompt).
        chk = run_limited([self.bin, "check", "main.nyra", "--json"], cwd=wd, env=env, timeout=CHECK_TIMEOUT)
        if chk.spawn_error:
            raise HarnessError(f"cannot run the Nyra compiler {self.bin}: {chk.spawn_error}")
        out = chk.stdout.decode("utf-8", "replace").strip()
        parsed = _loads(out)
        if not isinstance(parsed, dict) or chk.returncode not in (0, 1):
            detail = scrub_paths(chk.stderr.decode("utf-8", "replace") or out, wd).strip()
            return EvalResult(False, "toolchain_error", feedback=fb_toolchain(detail), stderr=detail,
                              exit_code=chk.returncode)
        if not parsed.get("ok", False):
            return EvalResult(False, "compile_error", feedback=fb_compile("The Nyra compiler (`nyra check --json`)", out),
                              errors=parsed.get("errors", []), stdout=out, exit_code=chk.returncode)
        target = "main.js" if js else ("prog.exe" if os.name == "nt" else "prog")
        build = run_limited([self.bin, "build", "main.nyra", "-o", target] + (["--js"] if js else []),
                            cwd=wd, env=env, timeout=BUILD_TIMEOUT)
        compile_ms = (time.perf_counter() - started) * 1000
        if build.returncode != 0 or not (wd / target).exists():
            detail = scrub_paths(build.stderr.decode("utf-8", "replace"), wd).strip()
            return EvalResult(False, "toolchain_error", feedback=fb_toolchain(detail), stderr=detail,
                              exit_code=build.returncode, compile_ms=compile_ms)
        return Built([self.node, target] if js else [wd / target], compile_ms)

    def self_repair(self, code: str, task: Task) -> dict:
        """`nyra check --fix`: the compiler repairs the mistakes whose fix is unambiguous, without a model call (zero
        tokens). Returns what happened: {"tried": True, "fixed": n, "changed": bool, "remaining": error codes left,
        "result": verdict of the repaired program or None, "code": the repaired program, "detail": the compiler's
        message when nothing was run}. The model's own repair loop is not affected."""
        with scratch_dir() as wd:
            env = child_env(wd)
            write_source(wd / "main.nyra", code)
            before = (wd / "main.nyra").read_bytes()
            fix = run_limited([self.bin, "check", "main.nyra", "--fix", "--json"], cwd=wd, env=env,
                              timeout=CHECK_TIMEOUT)
            if fix.spawn_error:
                raise HarnessError(f"cannot run the Nyra compiler {self.bin}: {fix.spawn_error}")
            after = (wd / "main.nyra").read_bytes()
            parsed = _loads(fix.stdout.decode("utf-8", "replace").strip())
            fixed = parsed.get("fixed", 0) if isinstance(parsed, dict) else 0
            out = {"tried": True, "fixed": fixed, "changed": after != before, "result": None, "detail": None,
                   "remaining": [e.get("code") for e in (parsed.get("errors") or [])] if isinstance(parsed, dict) else []}
            if fix.returncode != 0 or after == before or not (isinstance(parsed, dict) and parsed.get("ok")):
                out["detail"] = scrub_paths(fix.stderr.decode("utf-8", "replace"), wd).strip()[-300:] or None
                return out
            repaired = after.decode("utf-8", "replace")
        result = self.evaluate(repaired, task, timed=False)
        out["code"] = repaired
        out["result"] = result.to_dict()
        return out


def find_node(explicit: Optional[str] = None) -> str:
    """--node if given, else `node` from PATH. Returns something run_limited can start."""
    if explicit:
        found = shutil.which(explicit) or (explicit if Path(explicit).is_file() else None)
        if not found:
            raise HarnessError(f"--node {explicit}: no such program")
        return str(found)
    found = shutil.which("node")
    if not found:
        raise HarnessError("Node.js not found (it runs the TypeScript programs; the JavaScript backend of Nyra needs "
                           f"it too). Install Node.js {NODE_MIN_VERSION[0]}.{NODE_MIN_VERSION[1]} or newer, or "
                           "pass --node PATH, or leave TypeScript out with --langs.")
    return found


class TypeScriptLang(Language):
    """TypeScript through Node.js itself: the type annotations are erased (not checked) and the file runs.

    Node 22.6+ can do that behind a flag (22.18+ and 23.6+ by default); the flags used also accept enums,
    namespaces and constructor parameter properties. Nothing here compiles or type-checks, so a program that
    `tsc` would reject for a type error runs (and passes) as long as the JavaScript underneath is right.
    """

    name = "typescript"
    display = "TypeScript"
    ext = ".ts"
    fence_tags = ("typescript", "ts")
    hello_world = "const answer: number = 42;\nconsole.log(answer);\n"

    def __init__(self, node: Optional[str] = None, timeout: float = 10.0, time_runs: int = 0,
                 checker: Optional[typecheck.Checker] = None):
        super().__init__(timeout, time_runs)
        self.node = find_node(node)
        self.checker = checker
        self._version_text: Optional[str] = None

    @property
    def system_prompt(self) -> str:
        if self.checker is None:
            return _TYPESCRIPT_SYSTEM
        return (_TYPESCRIPT_SYSTEM.replace("which removes the type annotations without checking them, and it may",
                                           "after the type annotations have been checked, and it may")
                .replace(_task_paragraph("TypeScript"), _task_paragraph("TypeScript") + self.type_note()))

    def version_text(self) -> str:
        """Output of `node --version`, e.g. "v25.2.1" ("" if it cannot be read)."""
        if self._version_text is None:
            p = run_limited([self.node, "--version"], cwd=REPO_DIR, env=dict(os.environ), timeout=30)
            self._version_text = (p.stdout + p.stderr).decode("utf-8", "replace").strip()
        return self._version_text

    def version(self) -> Optional[tuple]:
        m = re.search(r"(\d+)\.(\d+)", self.version_text())
        return (int(m.group(1)), int(m.group(2))) if m else None

    def preflight(self) -> list:
        ver = self.version()
        if ver is None:
            raise HarnessError(f"{self.node} does not look like Node.js (`--version` printed {self.version_text()!r})")
        if ver < NODE_MIN_VERSION:
            raise HarnessError(f"Node.js {self.version_text()} cannot run TypeScript files; "
                               f"{NODE_MIN_VERSION[0]}.{NODE_MIN_VERSION[1]} or newer is needed")
        return super().preflight()

    def diagnose(self, stderr: str, stdout: str) -> Optional[str]:
        return _node_diagnose(stderr, stdout)

    def clean_stderr(self, stderr: str) -> str:
        return clean_node_stderr(stderr)

    def build(self, code: str, wd: Path, env: dict):
        write_source(wd / "main.ts", code)
        rejected = self.run_checker("main.ts", wd, env)  # --ts-typecheck: tsc, before Node.js strips the types and runs it
        if rejected is not None:
            return rejected
        return Built([self.node, *NODE_TS_FLAGS, "main.ts"])


def _find_rust_tool(name: str) -> Optional[str]:
    """`name` from PATH, else from rustup's proxy directory (~/.cargo/bin, or $CARGO_HOME/bin), which is where
    rustup puts rustc and where a shell started without the user's PATH settings does not look."""
    found = shutil.which(name)
    if found:
        return found
    home = Path(os.environ.get("CARGO_HOME") or (Path.home() / ".cargo"))
    path = home / "bin" / (name + (".exe" if os.name == "nt" else ""))
    return str(path) if path.is_file() else None


def _rustup_toolchains() -> list:
    rustup = _find_rust_tool("rustup")
    if not rustup:
        return []
    p = run_limited([rustup, "toolchain", "list"], cwd=REPO_DIR, env=dict(os.environ), timeout=30)
    if p.spawn_error or p.returncode != 0:
        return []
    return [ln.split()[0] for ln in p.stdout.decode("utf-8", "replace").splitlines() if ln.strip()]


def rustc_candidates(explicit: Optional[str] = None, windows: Optional[bool] = None) -> list:
    """The commands to try for compiling Rust, best first (each is an argv prefix such as ["rustc", "+toolchain"]).

    --rustc wins and is the only candidate. Otherwise: the rustc that is installed, and, on Windows, first
    every installed GNU toolchain. The default Windows toolchain is MSVC, which cannot link without the
    Visual Studio build tools (error: linking with `link.exe` failed); the GNU toolchain links with the MinGW
    that rustup ships, so it works on a machine that has no Visual Studio. The first candidate that
    compiles and runs a hello-world program is the one used.
    """
    windows = (os.name == "nt") if windows is None else windows
    if explicit:
        parts = shlex.split(explicit, posix=not windows)
        return [[p.strip('"') for p in parts]]
    rustc = _find_rust_tool("rustc")
    if rustc is None:
        return []
    candidates = [[rustc]]
    if windows:
        candidates = [[rustc, f"+{name}"] for name in _rustup_toolchains() if "windows-gnu" in name] + candidates
    return candidates


def _command_label(cmd: list) -> str:
    """`rustc +stable-x86_64-pc-windows-gnu` (no directories: result files get committed)."""
    # split on both separators: a Windows path must give the same label on every platform
    name = cmd[0].replace("\\", "/").rsplit("/", 1)[-1]
    if name.lower().endswith(".exe"):
        name = name[:-4]
    return " ".join([name] + list(cmd[1:]))


class RustLang(Language):
    """Rust through rustc: `rustc -O --edition 2021 main.rs`, then the executable it produced."""

    name = "rust"
    display = "Rust"
    ext = ".rs"
    fence_tags = ("rust", "rs")
    hello_world = 'fn main() {\n    println!("42");\n}\n'

    def __init__(self, rustc: Optional[str] = None, timeout: float = 10.0, time_runs: int = 0):
        super().__init__(timeout, time_runs)
        self.explicit = rustc
        self.cmd: Optional[list] = None  # the working command; found lazily (preflight or the first evaluate)
        self._version_text: Optional[str] = None
        self._lock = threading.Lock()

    @property
    def system_prompt(self) -> str:
        return _RUST_SYSTEM

    def command(self) -> list:
        with self._lock:
            if self.cmd is None:
                self.cmd = self._probe()
            return self.cmd

    def _probe(self) -> list:
        candidates = rustc_candidates(self.explicit)
        if not candidates:
            raise HarnessError("rustc not found (it compiles the Rust programs). Install Rust from https://rustup.rs "
                               "(it is looked for on PATH and in ~/.cargo/bin), or pass --rustc COMMAND, or leave "
                               "Rust out with --langs.")
        hello = Task("preflight", "", "", "42\n", "0.1", "", "", Path("."))
        failures = []
        for cmd in candidates:
            result = self._evaluate_with(cmd, self.hello_world, hello)
            if result.passed:
                return cmd
            failures.append(f"`{_command_label(cmd)}`: {result.kind}: "
                            f"{(result.stderr or result.stdout).strip()[:300]}")
        raise HarnessError("Rust toolchain self-test failed. Tried " + "; ".join(failures) + ". On Windows without "
                           "the Visual Studio build tools use the GNU toolchain: `rustup toolchain install "
                           "stable-x86_64-pc-windows-gnu`, or pass --rustc 'rustc +stable-x86_64-pc-windows-gnu'.")

    def version_text(self) -> str:
        """`rustc --version` of the command in use, e.g. "rustc 1.99.0 (b940084d7 2026-09-28)"."""
        if self._version_text is None:
            p = run_limited([*self.command(), "--version"], cwd=REPO_DIR, env=dict(os.environ), timeout=30)
            self._version_text = (p.stdout + p.stderr).decode("utf-8", "replace").strip()
        return self._version_text

    def command_text(self) -> str:
        return _command_label(self.command())

    def preflight(self) -> list:
        self.command()  # raises HarnessError with the reason if no toolchain works
        return []

    def evaluate(self, code: str, task: Task) -> EvalResult:
        return self._evaluate_with(self.command(), code, task)

    def build(self, code: str, wd: Path, env: dict):
        return self._build_with(self.command(), code, wd, env)

    def _evaluate_with(self, cmd: list, code: str, task: Task) -> EvalResult:
        with scratch_dir() as wd:
            env = child_env(wd)
            built = self._build_with(cmd, code, wd, env)
            if isinstance(built, EvalResult):
                return built
            return self.run_built(built, task, wd, env)

    def _build_with(self, cmd: list, code: str, wd: Path, env: dict):
        write_source(wd / "main.rs", code)
        target = "prog.exe" if os.name == "nt" else "prog"
        build = run_limited([*cmd, *RUSTC_FLAGS, "main.rs", "-o", target], cwd=wd, env=env, timeout=BUILD_TIMEOUT)
        if build.spawn_error:
            raise HarnessError(f"cannot run `{_command_label(cmd)}`: {build.spawn_error}")
        compile_ms = build.elapsed * 1000
        detail = scrub_paths(build.stderr.decode("utf-8", "replace"), wd).strip()
        if build.timed_out:
            return EvalResult(False, "toolchain_error", feedback=fb_toolchain(detail), stderr=detail,
                              exit_code=build.returncode, compile_ms=compile_ms)
        if build.returncode != 0 or not (wd / target).exists():
            # Exit 1 with `error[E0425]: ...` is the program's fault; a failed link, a missing component or
            # an internal compiler error is the toolchain's (and says nothing about the program).
            if _is_rustc_toolchain_failure(detail) or build.returncode not in (0, 1):
                return EvalResult(False, "toolchain_error", feedback=fb_toolchain(detail), stderr=detail,
                                  exit_code=build.returncode, compile_ms=compile_ms)
            return EvalResult(False, "compile_error", feedback=fb_compile("The Rust compiler (`rustc`)", detail),
                              stderr=detail, exit_code=build.returncode, compile_ms=compile_ms)
        return Built([wd / target], compile_ms)


_RUSTC_TOOLCHAIN_FAILURES = ("error: linking with", "error: linker", "could not exec the linker", "internal compiler error",
                             "rustup could not choose", "error: toolchain", "error: could not find", "no override and no default",
                             "cannot find the file specified")


def _is_rustc_toolchain_failure(stderr: str) -> bool:
    low = stderr.lower()
    return any(marker in low for marker in _RUSTC_TOOLCHAIN_FAILURES)


def _loads(text: str):
    try:
        return json.loads(text)
    except ValueError:
        return None


def find_nyra(explicit: Optional[str] = None) -> Path:
    """--nyra if given, else target/release/nyra(.exe), else target/debug/nyra(.exe)."""
    exe = "nyra.exe" if os.name == "nt" else "nyra"
    if explicit:
        path = Path(explicit)
        if not path.is_file():
            found = shutil.which(explicit)
            if not found:
                raise HarnessError(f"--nyra {explicit}: no such file")
            path = Path(found)
        return path
    for profile in ("release", "debug"):
        path = REPO_DIR / "target" / profile / exe
        if path.is_file():
            return path
    raise HarnessError("Nyra compiler not found (looked for target/release and target/debug). Build it with "
                       "`cargo build --release` (on Windows without MSVC: `cargo +stable-x86_64-pc-windows-gnu "
                       "build --release`) or pass --nyra PATH.")


EDIT_ARMS = DEFAULT_LANGS["edit"]


def canonical_lang(name: str) -> str:
    name = name.strip().lower()
    return LANG_ALIASES.get(name, name)


def make_languages(names: list, *, nyra: Optional[str], backend: str, spec: Path, timeout: float,
                   node: Optional[str] = None, rustc: Optional[str] = None, time_runs: int = 0,
                   python_checker: Optional[typecheck.Checker] = None,
                   ts_checker: Optional[typecheck.Checker] = None) -> dict:
    langs = {}
    for name in names:
        if name == "nyra":
            langs[name] = NyraLang(find_nyra(nyra), backend=backend, spec_path=spec, timeout=timeout,
                                   node=find_node(node) if backend == "js" else "node", time_runs=time_runs)
        elif name == "python":
            langs[name] = PythonLang(timeout=timeout, time_runs=time_runs, checker=python_checker)
        elif name == "typescript":
            langs[name] = TypeScriptLang(node, timeout=timeout, time_runs=time_runs, checker=ts_checker)
        elif name == "rust":
            langs[name] = RustLang(rustc, timeout=timeout, time_runs=time_runs)
        elif name in EDIT_ARMS:  # the edit tier: ways of changing a program (bench/edit_arms.py)
            import edit_arms
            cls = edit_arms.ARM_CLASSES[name]
            if name == "nyra-edit":
                langs[name] = cls(find_nyra(nyra), backend=backend, spec_path=spec, timeout=timeout,
                                  node=find_node(node) if backend == "js" else "node", time_runs=time_runs)
            else:
                langs[name] = cls(timeout=timeout, time_runs=time_runs, checker=python_checker)
        else:
            raise UsageError(f"unknown language {name!r}; available: {', '.join(LANG_ORDER)} "
                             f"(and, for --tier edit, {', '.join(EDIT_ARMS)})")
    return langs


def nyra_of(langs: dict) -> Optional["NyraLang"]:
    """The Nyra language of a run: `nyra`, or the `nyra-edit` arm of the edit tier."""
    return next((lang for lang in langs.values() if isinstance(lang, NyraLang)), None)


# ------------------------------------------------------------------------ one attempt loop


@dataclasses.dataclass
class RunContext:
    provider: providers.Provider
    repairs: int
    count_tokens: bool
    abort: threading.Event = dataclasses.field(default_factory=threading.Event)
    fatal: list = dataclasses.field(default_factory=list)
    over_budget: Optional[Callable[[], bool]] = None  # --budget: called after every reply; True stops the run
    self_repair: bool = False  # try `nyra check --fix` on a Nyra first attempt that does not compile


def run_one(task: Task, lang: Language, sample: int, ctx: RunContext) -> dict:
    """Give one task to the model in one language; allow up to `repairs` repair rounds.

    Returns the full record: every reply, the extracted program, the verdict and the feedback.
    status: pass (correct within the budget) | fail | error (provider/harness problem, excluded from
    the metrics) | aborted.
    """
    system = lang.system_prompt_for(task)
    messages = [{"role": "user", "content": lang.prompt_for(task)}]
    attempts: list = []
    status = "fail"
    error: Optional[str] = None
    for n in range(1, ctx.repairs + 2):
        if ctx.abort.is_set():
            status = "aborted"
            break
        meta = {"task_id": task.id, "lang": lang.name, "attempt": n, "sample": sample,
                "example_output": task.example.expected_output if task.example else None}  # (mock provider only)
        try:
            reply = ctx.provider.complete(system, messages, meta)
        except providers.ProviderError as exc:
            if exc.fatal:
                ctx.fatal.append(exc)
                ctx.abort.set()
            status, error = "error", str(exc)
            break
        if ctx.over_budget is not None and ctx.over_budget():
            ctx.abort.set()  # the money is spent: finish this reply, start nothing new
        code = lang.extract(reply.text, task)
        attempt = {"n": n, "reply": reply.text, "stop_reason": reply.stop_reason, "usage": reply.usage.to_dict(),
                   "latency_s": round(reply.latency_s, 3), "request_id": reply.request_id,
                   "served_model": reply.model, "served_by": reply.upstream, "code": code, "chars": None,
                   "lines": None, "code_tokens": None}
        if code is None:
            result = EvalResult(False, "no_code", feedback=lang.no_code_feedback())
        else:
            attempt["chars"], attempt["lines"] = code_size(code)
            # Only the first attempt's code tokens enter the report; a provider that bills for counting
            # (openrouter) is not asked about the repairs.
            if ctx.count_tokens and (n == 1 or ctx.provider.count_all_attempts):
                attempt["code_tokens"] = ctx.provider.count_tokens(code)
            try:
                result = lang.evaluate(code, task)
                if n == 1 and ctx.self_repair and isinstance(lang, NyraLang):
                    # pass@1 with self-repair: the compiler's own --fix, once, at zero token cost. Measured on the
                    # side; the model still gets its normal feedback and repair attempts.
                    attempt["self_repair"] = (lang.self_repair(code, task) if result.kind == "compile_error"
                                              else {"tried": False})
            except HarnessError as exc:
                ctx.fatal.append(exc)
                ctx.abort.set()
                attempts.append(attempt)
                status, error = "error", str(exc)
                break
        attempt["result"] = result.to_dict()
        attempts.append(attempt)
        if result.passed:
            status = "pass"
            break
        if n <= ctx.repairs:
            attempt["feedback"] = result.feedback
            messages.append({"role": "assistant", "content": reply.text.strip() or "(empty reply)"})
            messages.append({"role": "user", "content": result.feedback})
    return {
        "task_id": task.id, "lang": lang.name, "sample": sample, "status": status,
        "first_try": bool(attempts and attempts[0].get("result", {}).get("passed")),
        "attempts_used": len(attempts), "attempts": attempts, "error": error,
    }


# ----------------------------------------------------------------------- orchestration


def select_tasks(tasks: list, patterns: Optional[str], max_version: Optional[tuple], lang_names: list,
                 provider: providers.Provider) -> tuple:
    """Apply --tasks, --max-version and (for the mock) reference availability.

    Every language runs the same tasks: a task is dropped for all of them or for none.
    Returns (selected, excluded) where excluded is a list of {"id", "reason"}.
    """
    chosen = tasks
    if patterns:
        wanted = [p.strip() for p in patterns.split(",") if p.strip()]
        hit = set()
        for pattern in wanted:
            matches = {t.id for t in tasks if fnmatch.fnmatchcase(t.id, pattern)}
            if not matches:
                raise UsageError(f"--tasks: no task matches {pattern!r} (--dry-run lists the tasks)")
            hit |= matches
        chosen = [t for t in tasks if t.id in hit]
    selected, excluded = [], []
    for t in chosen:
        if max_version is not None and t.version > max_version:
            excluded.append({"id": t.id, "reason": f"needs Nyra {t.min_version} (limit is {max_version[0]}.{max_version[1]})"})
            continue
        if provider.is_mock:
            missing = [n for n in lang_names if not provider.has_reference(n, t.id)]
            if missing:
                excluded.append({"id": t.id, "reason": f"no reference solution for {', '.join(missing)}"})
                continue
        selected.append(t)
    return selected, excluded


def execute(jobs: list, ctx: RunContext, n_workers: int, quiet: bool) -> tuple:
    """Run all (task, lang, sample) jobs on a thread pool. Returns (records, interrupted)."""
    records: list = []
    lock = threading.Lock()
    total = len(jobs)
    interrupted = False

    def work(job):
        task, lang, sample = job
        if ctx.abort.is_set():
            return {"task_id": task.id, "lang": lang.name, "sample": sample, "status": "aborted", "first_try": False,
                    "attempts_used": 0, "attempts": [], "error": None}
        return run_one(task, lang, sample, ctx)

    pool = cf.ThreadPoolExecutor(max_workers=max(1, n_workers))
    futures = [pool.submit(work, job) for job in jobs]
    try:
        for fut in cf.as_completed(futures):
            try:
                rec = fut.result()
            except Exception as exc:  # a bug in the harness itself: do not lose the other results
                ctx.abort.set()
                raise HarnessError(f"internal error: {type(exc).__name__}: {exc}") from exc
            with lock:
                records.append(rec)
                if not quiet:
                    print(_progress_line(len(records), total, rec), flush=True)
    except KeyboardInterrupt:
        interrupted = True
        ctx.abort.set()
        print("\ninterrupted: saving what has finished so far", file=sys.stderr)
    finally:
        pool.shutdown(wait=True, cancel_futures=True)
    return records, interrupted


def _progress_line(done: int, total: int, rec: dict) -> str:
    if rec["status"] == "pass":
        verdict = "pass" + ("" if rec["attempts_used"] == 1 else f" after {rec['attempts_used']} attempts")
    elif rec["status"] == "fail":
        last = rec["attempts"][-1]["result"]["kind"] if rec["attempts"] else "?"
        verdict = f"FAIL ({last}) after {rec['attempts_used']} attempts"
    else:
        verdict = f"{rec['status'].upper()}: {rec.get('error') or ''}".strip()
    return f"[{done:>3}/{total}] {rec['lang']:<10} {rec['task_id']:<22} {verdict}"


def _git_info() -> dict:
    def git(*args):
        try:
            p = subprocess.run(["git", "-C", str(REPO_DIR), *args], capture_output=True, text=True, timeout=15)
            return p.stdout.strip() if p.returncode == 0 else None
        except (OSError, subprocess.TimeoutExpired):
            return None
    commit = git("rev-parse", "--short", "HEAD")
    status = git("status", "--porcelain", "--", ".", ":(exclude)bench/results")
    return {"commit": commit, "dirty": bool(status) if status is not None else None}


def _safe(name: str) -> str:
    return re.sub(r"[^A-Za-z0-9._-]+", "-", name).strip("-") or "unnamed"


def display_path(path: Path) -> str:
    """Repo-relative path if the file is inside the repo, else just its name (result files get committed,
    so they must not carry local directory names)."""
    try:
        return Path(path).resolve().relative_to(REPO_DIR).as_posix()
    except ValueError:
        return Path(path).name


def result_paths(out_dir: Path, date: str, provider_name: str, model: str) -> tuple:
    stem = f"{date}-{_safe(provider_name)}-{_safe(model)}"
    candidate, n = out_dir / f"{stem}.json", 1
    while candidate.exists():  # never overwrite an earlier run
        n += 1
        candidate = out_dir / f"{stem}-{n}.json"
    return candidate, candidate.with_suffix(".md")


def toolchain_info(langs: dict) -> dict:
    """Versions of the tools that run the programs, for the result file."""
    info: dict = {"node": None, "rust": None}
    ts = langs.get("typescript")
    nyra = langs.get("nyra")
    if ts is not None:
        info["node"] = {"version": ts.version_text(), "flags": " ".join(NODE_TS_FLAGS)}
    elif nyra is not None and nyra.backend == "js":
        info["node"] = {"version": run_limited([nyra.node, "--version"], cwd=REPO_DIR, env=dict(os.environ),
                                               timeout=30).stdout.decode("utf-8", "replace").strip(), "flags": ""}
    rust = langs.get("rust")
    if rust is not None:
        info["rust"] = {"command": rust.command_text(), "version": rust.version_text(), "flags": " ".join(RUSTC_FLAGS)}
    return info


def timing_info(plan) -> dict:
    """How runtimes were measured, for the result file: timed runs per passing program, and each language's
    start-up time (the median run time of its hello-world program), which is subtracted from the median."""
    return {"runs": plan.args.time_runs, "statistic": "median", "jobs": plan.args.jobs,
            "startup_ms": {n: (None if plan.langs[n].measured_startup_ms() is None
                               else round(plan.langs[n].measured_startup_ms(), 2)) for n in plan.lang_names}}


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="run.py", description=__doc__.split("\n\n")[0],
        epilog="Methodology and metrics: bench/README.md",
        formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--provider", default="mock", choices=sorted(providers.PROVIDERS),
                   help="mock replays the reference solutions (no API key); anthropic calls Claude (ANTHROPIC_API_KEY); "
                        "openrouter calls any model on OpenRouter (OPENROUTER_API_KEY) (default: mock)")
    p.add_argument("--model", help="model id (anthropic default: %s; mock default: mock; openrouter has no default)"
                   % providers.AnthropicProvider.default_model)
    p.add_argument("--models", help="comma-separated model ids to run one after the other, e.g. "
                                    "anthropic/claude-opus-5.5,openai/gpt-6-sol; `default` is the list in "
                                    "bench/models.json; with mock: mock,mock-flaky,mock-wrong")
    p.add_argument("--langs", default=None,
                   help="comma-separated languages to run: nyra, python, typescript (ts), rust (rs) "
                        "(default: %s; the edit tier runs its arms %s instead). The first is the baseline of the paired "
                        "comparisons." % (",".join(LANG_ORDER), ",".join(DEFAULT_LANGS["edit"])))
    p.add_argument("--tier", choices=TIERS, default=None,
                   help="which task set: v1, the original input-free tasks (default); v2, tasks that read stdin and are "
                        "judged on hidden inputs; edit, change an existing program (arms %s); safety, tasks whose "
                        "naive solution reads files or environment variables it should not (pending, see "
                        "bench/README.md)" % ", ".join(DEFAULT_LANGS["edit"]))
    p.add_argument("--hidden-dir", metavar="DIR",
                   help="v2 tier: a private folder of <task id>.json files holding extra hidden `cases` that are not in "
                        "the repository; a program must pass those as well (so published tasks cannot be memorized)")
    p.add_argument("--preset", metavar="NAME",
                   help="a bundle from bench/models.json (the `cheap` models, 5 samples, the v2 tier): fills in whatever "
                        "--models, --samples, --repairs and --tier leave out")
    p.add_argument("--python-typecheck", nargs="?", const="auto", metavar="TOOL",
                   help="type-check the Python programs with mypy or pyright before running them (auto, mypy or pyright; "
                        "skipped with a message when neither is installed) and tell the model so")
    p.add_argument("--ts-typecheck", nargs="?", const="auto", metavar="TOOL",
                   help="type-check the TypeScript programs with tsc before running them (skipped with a message when "
                        "tsc is not installed); without it the TypeScript arm is not type-checked")
    p.add_argument("--include-pending", action="store_true",
                   help="safety tier: also run the tasks that are still marked pending (the compiler has no --allow "
                        "capability flag yet)")
    p.add_argument("--tasks", help="comma-separated task ids or patterns such as 'fizz*' (default: all)")
    p.add_argument("--max-version", help="skip tasks needing a newer Nyra than this, for every language, e.g. 0.1 "
                                         "(default: the version of the nyra compiler when nyra is run)")
    p.add_argument("--repairs", type=int, default=None, help="repair attempts after a failed first try (default: 3)")
    p.add_argument("--samples", type=int, default=None, help="independent runs per task and language (default: 1)")
    p.add_argument("--nyra", help="path to the nyra binary (default: target/release, else target/debug)")
    p.add_argument("--backend", default="native", choices=("native", "js"),
                   help="Nyra backend that runs the programs (default: native, via the C compiler)")
    p.add_argument("--spec", default=str(DEFAULT_SPEC), help="Nyra spec shown to the model (default: docs/SPEC.md)")
    p.add_argument("--node", help="the node program that runs TypeScript (default: node on PATH; Node.js %d.%d or newer)"
                   % NODE_MIN_VERSION)
    p.add_argument("--rustc", help="the Rust compiler command, e.g. 'rustc +stable-x86_64-pc-windows-gnu' "
                                   "(default: found automatically; on Windows the GNU toolchain is preferred)")
    p.add_argument("--timeout", type=float, default=10.0, help="seconds a program may run (default: 10)")
    p.add_argument("--time-runs", type=int, default=DEFAULT_TIME_RUNS, metavar="N",
                   help="run every passing program N more times and keep the median run time, minus the language's "
                        "start-up time (default: %d; 0: no timing)" % DEFAULT_TIME_RUNS)
    p.add_argument("--jobs", type=int, default=4, help="tasks evaluated in parallel (default: 4)")
    p.add_argument("--out", default=str(RESULTS_DIR), help="directory for the result files (default: bench/results)")
    p.add_argument("--max-tokens", type=int, default=16000,
                   help="anthropic, openrouter: max_tokens per reply; thinking shares it (default: 16000)")
    p.add_argument("--effort", choices=("none", "minimal", "low", "medium", "high", "xhigh", "max"),
                   help="anthropic: output_config.effort; openrouter: reasoning.effort "
                        "(default: the model's own default)")
    p.add_argument("--extra-json", help="anthropic, openrouter: JSON object of extra request fields, e.g. "
                                        "'{\"reasoning\": {\"max_tokens\": 2000}}' or "
                                        "'{\"provider\": {\"order\": [\"anthropic\"], \"allow_fallbacks\": false}}'")
    p.add_argument("--base-url", help="openrouter: another OpenAI-compatible endpoint (https only; for tests and proxies)")
    p.add_argument("--no-model-check", action="store_true",
                   help="openrouter: do not check the model ids against OpenRouter's public list before starting")
    p.add_argument("--budget", type=float, metavar="USD",
                   help="openrouter: stop the whole run once the calls so far have cost this many dollars")
    p.add_argument("--assume-output-tokens", type=int, default=1500, metavar="N",
                   help="--dry-run cost estimate: output tokens per attempt, thinking included (default: 1500)")
    p.add_argument("--no-self-repair", action="store_true",
                   help="do not try `nyra check --fix` on Nyra first attempts that do not compile (the \"pass@1 with "
                        "self-repair\" metric; it never changes the other metrics)")
    p.add_argument("--no-count-tokens", action="store_true",
                   help="do not measure code-only tokens with the provider's token counter")
    p.add_argument("--mock-flaky", nargs="?", const="mix", choices=("mix",) + providers.DEFECTS + providers.EXTRA_DEFECTS,
                   help="mock only: break the first attempt of some tasks to exercise the repair loop")
    p.add_argument("--dry-run", action="store_true",
                   help="print what would run (and the maximum number of API calls, and an estimated cost) and exit")
    p.add_argument("-q", "--quiet", action="store_true", help="no per-task progress lines")
    return p


def main(argv: Optional[list] = None) -> int:
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8", errors="replace")
    args = build_parser().parse_args(argv)
    try:
        return _main(args)
    except UsageError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    except HarnessError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    except providers.ProviderError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2


def resolve_models(args) -> list:
    """The model ids to run, in order. openrouter has no default: it never guesses which models to pay for."""
    if args.model and args.models:
        raise UsageError("use --model or --models, not both")
    raw = args.models or args.model
    if not raw:
        default = providers.PROVIDERS[args.provider].default_model
        if not default:
            raise UsageError(f"--provider {args.provider} has no default model: pass --models a,b,c "
                             "(`python bench/models.py claude` finds OpenRouter ids, `--models default` uses "
                             "bench/models.json)")
        return [default]
    ids: list = []
    for item in [x.strip() for x in raw.split(",") if x.strip()]:
        if item == "default":
            if args.provider != "openrouter":
                raise UsageError("--models default is the OpenRouter list of bench/models.json: use it with "
                                 "--provider openrouter")
            try:
                ids += modelsmod.default_model_ids()
            except modelsmod.ModelsError as exc:
                raise UsageError(str(exc)) from None
        else:
            ids.append(item)
    if not ids:
        raise UsageError("no model ids given")
    dupes = sorted({m for m in ids if ids.count(m) > 1})
    if dupes:
        raise UsageError(f"model listed more than once: {', '.join(dupes)}")
    return ids


def check_openrouter_models(ids: list, base_url: Optional[str] = None, max_tokens: Optional[int] = None) -> Optional[list]:
    """Refuse unknown OpenRouter ids before anything is spent, and warn about a --max-tokens a model would reject.
    Returns the public listing (None if it could not be fetched)."""
    try:
        listing = modelsmod.fetch_models(base_url or providers.OPENROUTER_BASE_URL)
    except (modelsmod.ModelsError, ValueError) as exc:
        print(f"warning: could not check the model ids against OpenRouter's public list ({exc}); "
              "an unknown id will be rejected by the first request instead", file=sys.stderr)
        return None
    bad = modelsmod.missing_ids(listing, ids)
    if bad:
        detail = "; ".join(f"{mid}" + (f" (similar: {', '.join(near)})" if near else "") for mid, near in bad)
        raise UsageError(f"not an OpenRouter model id: {detail}. `python bench/models.py WORD` searches the list "
                         "(--no-model-check skips this check).")
    by_id = {m["id"]: m for m in listing}
    for mid in ids:
        limit = ((modelsmod.lookup(by_id, mid) or {}).get("top_provider") or {}).get("max_completion_tokens")
        if max_tokens and isinstance(limit, int) and not isinstance(limit, bool) and max_tokens > limit:
            print(f"warning: --max-tokens {max_tokens} is above the {limit} completion tokens {mid} allows, so its "
                  f"requests will probably be rejected; pass --max-tokens {limit} or less", file=sys.stderr)
    return listing


ASSUMED_ATTEMPTS = 1.3  # attempts per run in the --dry-run cost estimate (first try mostly, some repairs)


def estimate_cost(price: tuple, tasks: list, lang_names: list, langs: dict, samples: int, output_tokens: int) -> float:
    """Rough dollars for one model: input from the real prompts (about 3.5 characters per token, repair history
    ignored), output from an assumption (thinking models spend several times what the program itself needs)."""
    runs = len(tasks) * len(lang_names) * samples
    chars = sum(len(langs[n].system_prompt) + len(t.prompt) for t in tasks for n in lang_names) * samples
    input_tokens = (chars / 3.5 + 30 * runs) * ASSUMED_ATTEMPTS
    return input_tokens * price[0] + runs * ASSUMED_ATTEMPTS * output_tokens * price[1]


class BudgetGuard:
    """--budget: stop the run once the providers have reported this many dollars of cost in total."""

    def __init__(self, limit: float, plist: list):
        self.limit = limit
        self.providers = plist
        self.hit = False

    def spent(self) -> float:
        return sum(p.spent() or 0.0 for p in self.providers)

    def exceeded(self) -> bool:
        if self.spent() >= self.limit:
            self.hit = True
        return self.hit


@dataclasses.dataclass
class Plan:
    """Everything that is the same for every model of a run."""
    args: argparse.Namespace
    lang_names: list
    langs: dict
    tasks: list
    excluded: list
    max_version: Optional[tuple]
    max_source: Optional[str]
    warnings: list
    toolchains: dict
    typechecks: dict = dataclasses.field(default_factory=dict)  # what --python-typecheck / --ts-typecheck did
    hidden: Optional[dict] = None  # --hidden-dir: {"files", "sha256"}


@dataclasses.dataclass
class ModelOutcome:
    model: str
    results: Optional[dict] = None
    json_path: Optional[Path] = None
    md_path: Optional[Path] = None
    fatal: list = dataclasses.field(default_factory=list)
    interrupted: bool = False
    stop_all: bool = False  # the key or the account is the problem (or the budget is spent): no point in other models
    exit_code: int = 0


def run_model(plan: Plan, provider: providers.Provider, out_dir: Path, budget: Optional[BudgetGuard],
              single: bool) -> ModelOutcome:
    """Run every (task, language, sample) job for one model, write its result files, return what happened."""
    args = plan.args
    nyra = nyra_of(plan.langs)
    ctx = RunContext(provider=provider, repairs=args.repairs, count_tokens=not args.no_count_tokens,
                     over_budget=budget.exceeded if budget else None,
                     self_repair=not args.no_self_repair and args.tier != "edit")
    jobs = [(t, plan.langs[n], s) for t in plan.tasks for n in plan.lang_names for s in range(args.samples)]
    started = dt.datetime.now(dt.timezone.utc)
    records, interrupted = execute(jobs, ctx, args.jobs, args.quiet)
    finished = dt.datetime.now(dt.timezone.utc)
    outcome = ModelOutcome(model=provider.model, fatal=list(ctx.fatal), interrupted=interrupted)
    # A broken key or account, a broken local toolchain and a spent budget hurt every model alike.
    outcome.stop_all = (any(getattr(e, "stop_all", False) or isinstance(e, HarnessError) for e in ctx.fatal)
                        or bool(budget and budget.hit))

    order = {t.id: i for i, t in enumerate(plan.tasks)}
    records.sort(key=lambda r: (order[r["task_id"]], plan.lang_names.index(r["lang"]), r["sample"]))
    complete = not interrupted and not ctx.fatal and all(r["status"] != "aborted" for r in records)
    if not any(r["status"] in ("pass", "fail") for r in records):
        # Nothing usable (bad key, unknown model, ...): do not leave an empty result file behind.
        reason = ctx.fatal[0] if ctx.fatal else next((r["error"] for r in records if r.get("error")), "no run finished")
        print(f"error: no run finished for {provider.model}, so no result files were written: {reason}", file=sys.stderr)
        outcome.exit_code = 130 if interrupted else 2
        return outcome
    warnings = list(plan.warnings)
    served = sorted({a["served_model"] for r in records for a in r["attempts"] if a.get("served_model")})
    if len(served) > 1:
        warnings.append(f"the provider served more than one model id during the run: {', '.join(served)}")
    served_by = sorted({a["served_by"] for r in records for a in r["attempts"] if a.get("served_by")})
    spent = provider.spent()

    results = {
        "schema": SCHEMA_VERSION,
        "run": {
            "date": dt.date.today().isoformat(), "started_at": started.isoformat(timespec="seconds"),
            "finished_at": finished.isoformat(timespec="seconds"), "complete": complete,
            "provider": provider.describe(), "mock": provider.is_mock,
            "mock_flaky": args.mock_flaky if provider.is_mock else None,
            "tokens_are_estimates": provider.tokens_are_estimates,
            "langs": plan.lang_names, "repairs": args.repairs, "samples": args.samples, "timeout_s": args.timeout,
            "tier": args.tier, "preset": args.preset, "typecheck": plan.typechecks, "hidden_dir": plan.hidden,
            "extraction": ("the first fenced block of the reply" if args.tier == "v1" else
                           "the last fenced block tagged with the language (else the last block)"),
            "self_repair": None if nyra is None or args.tier == "edit" else not args.no_self_repair,
            "backend": nyra.backend if nyra else None, "jobs": args.jobs,
            "max_version": None if plan.max_version is None else f"{plan.max_version[0]}.{plan.max_version[1]}",
            "max_version_source": plan.max_source,
            "nyra": None if nyra is None else {"path": display_path(nyra.bin), "version": nyra.version_text()},
            "spec": None if nyra is None else {"path": display_path(nyra.spec_path), "version": nyra.spec_version,
                                               "sha256": nyra.spec_sha256},
            "node": plan.toolchains["node"], "rust": plan.toolchains["rust"],
            "timing": timing_info(plan),
            "python": platform.python_version(), "platform": platform.platform(),
            "served_models": served, "served_by": served_by, "spent_usd": spent, "budget_usd": args.budget,
            "repo": _git_info(), "tasks_sha256": tasks_digest(TIER_DIRS[args.tier], args.tier), "warnings": warnings,
            # Everything needed to rebuild what the model saw: system prompt + task prompt, then for each
            # attempt its reply and the feedback that followed it.
            "system_prompts": {n: plan.langs[n].system_prompt_for(plan.tasks[0]) for n in plan.lang_names},
            "tasks": {t.id: {"title": t.title, "category": t.category, "difficulty": t.difficulty,
                             "min_version": t.min_version, "prompt": t.prompt} for t in plan.tasks},
            "task_ids": [t.id for t in plan.tasks], "excluded_tasks": plan.excluded,
        },
        "records": records,
    }
    results["summary"] = report.summarize(records, plan.lang_names, {t.id: t.category for t in plan.tasks})
    markdown = report.render_markdown(results, {t.id: t for t in plan.tasks})
    if args.tier in ("v2", "edit"):  # hidden inputs and edits have numbers of their own (bench/tier_stats.py)
        import tier_stats
        results["summary"].update(tier_stats.tier_summary(args.tier, records, plan.lang_names))
        markdown += "\n" + tier_stats.render_tier(results, args.tier)

    outcome.results = results
    outcome.json_path, outcome.md_path = result_paths(out_dir, results["run"]["date"], provider.name, provider.model)
    outcome.json_path.write_text(json.dumps(results, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    outcome.md_path.write_text(markdown, encoding="utf-8")
    (out_dir / "latest.md").write_text(markdown, encoding="utf-8")

    if single:
        print()
        print(markdown)
        print(f"wrote {outcome.json_path}\n      {outcome.md_path}\n      {out_dir / 'latest.md'}")
    else:
        stats = results["summary"]["langs"]
        print(f"\n{provider.model}: pass@1 " + ", ".join(
            f"{report.display(n)} {stats[n]['pass_at_1']}/{stats[n]['n']}" for n in plan.lang_names)
            + (f"; spent ${spent:.4f}" if spent is not None else "") + f"\nwrote {outcome.json_path}")
    for exc in ctx.fatal[:1]:
        print(f"error: the run of {provider.model} was stopped by a fatal problem: {exc}", file=sys.stderr)
        outcome.exit_code = 2
    if interrupted:
        outcome.exit_code = 130
    bad = [r for r in records if r["status"] in ("error", "aborted")]
    if bad and not (budget and budget.hit):
        print(f"warning: {len(bad)} run(s) of {provider.model} ended in an error and are excluded from the metrics",
              file=sys.stderr)
    if budget and spent is None:
        print(f"warning: --budget could not work for {provider.model}: the API reported no costs", file=sys.stderr)
    if budget and budget.hit:
        print(f"warning: the budget of ${budget.limit:g} was reached (${budget.spent():.4f} spent)"
              + (f": {len(bad)} run(s) of {provider.model} were not started or finished, and the numbers cover only "
                 "the runs that did" if bad else ""), file=sys.stderr)
        if bad:
            outcome.exit_code = outcome.exit_code or 2
    if provider.is_mock and not outcome.exit_code and args.tier != "safety" and any(r["status"] != "pass" for r in records):
        print("error: the mock run is the pipeline self-test: every task must pass", file=sys.stderr)
        outcome.exit_code = 1
    return outcome


# ------------------------------------------------------------------ tiers, presets, type checks (v2 / edit / safety)


def resolve_settings(args) -> None:
    """Fill in what the command line leaves open: from `--preset` (bench/models.json), then from the defaults of the
    tier. Called once, before anything else looks at the arguments."""
    preset: dict = {}
    if args.preset:
        try:
            preset = modelsmod.load_preset(args.preset)
        except modelsmod.ModelsError as exc:
            raise UsageError(str(exc)) from None
    if args.tier is None:
        args.tier = preset.get("tier", "v1")
    if args.samples is None:
        args.samples = preset.get("samples", 1)
    if args.repairs is None:
        args.repairs = preset.get("repairs", 3)
    if preset and not args.model and not args.models and args.provider in ("anthropic", "openrouter"):
        ids = preset.get(args.provider) or []
        if not ids:
            raise UsageError(f"preset {args.preset!r} lists no models for --provider {args.provider}")
        args.models = ",".join(ids)
    if args.langs is None:
        args.langs = ",".join(DEFAULT_LANGS[args.tier])


def apply_hidden_dir(tasks: list, hidden_dir: Path) -> tuple:
    """--hidden-dir: add the private cases of <hidden_dir>/<task id>.json (a `cases` list of {stdin, expected_output})
    to the hidden cases of the tasks. Returns (tasks, info for the result file: how many files, and a hash of their
    content, but never their content or path)."""
    if not Path(hidden_dir).is_dir():
        raise UsageError(f"--hidden-dir {hidden_dir}: not a folder")
    out, used, digest = [], 0, hashlib.sha256()
    known = {t.id for t in tasks}
    for path in sorted(Path(hidden_dir).glob("*.json")):
        if path.stem not in known:
            raise UsageError(f"--hidden-dir: {path.name} does not belong to a task of this tier")
    for t in tasks:
        path = Path(hidden_dir) / f"{t.id}.json"
        if not t.cases or not path.is_file():
            out.append(t)
            continue
        try:
            data = json.loads(path.read_text(encoding="utf-8"))
            raw = data["cases"]
            extra = tuple(Case(stdin=c["stdin"], expected_output=c["expected_output"], visible=False, name=f"private{i + 1}")
                          for i, c in enumerate(raw))
            if not extra or any(not isinstance(c.stdin, str) or not c.expected_output.strip() for c in extra):
                raise ValueError("needs a non-empty list of cases with a stdin and a non-empty expected_output")
        except (OSError, ValueError, KeyError, TypeError, AttributeError) as exc:
            raise UsageError(f"--hidden-dir: {path.name}: {exc}") from None
        digest.update(path.name.encode())
        digest.update(path.read_bytes().replace(b"\r\n", b"\n"))
        used += 1
        out.append(dataclasses.replace(t, cases=t.cases + extra))
    return out, {"files": used, "sha256": digest.hexdigest() if used else None}


def resolve_typechecks(args) -> tuple:
    """(python checker, ts checker, the description for the result file, the messages to print). A checker that was
    asked for and is not installed is skipped with a clear message; the result file records that."""
    notes: list = []
    info = {"python": typecheck.metadata("python", None, None, ""), "typescript": typecheck.metadata("typescript", None, None, "")}
    py = ts = None
    try:
        if args.python_typecheck:
            py, msg = typecheck.find_python_checker(args.python_typecheck)
            info["python"] = typecheck.metadata("python", args.python_typecheck, py, msg)
            notes.append(msg)
        if args.ts_typecheck:
            ts, msg = typecheck.find_ts_checker(args.ts_typecheck)
            info["typescript"] = typecheck.metadata("typescript", args.ts_typecheck, ts, msg)
            notes.append(msg)
    except ValueError as exc:
        raise UsageError(str(exc)) from None
    return py, ts, info, notes


def _main(args) -> int:
    resolve_settings(args)
    lang_names = [canonical_lang(n) for n in args.langs.split(",") if n.strip()]
    if args.tier == "edit" and not all(n in EDIT_ARMS for n in lang_names):
        raise UsageError(f"--tier edit compares the arms {', '.join(EDIT_ARMS)}: use --langs with those")
    if args.tier != "edit" and any(n in EDIT_ARMS for n in lang_names):
        raise UsageError(f"{', '.join(n for n in lang_names if n in EDIT_ARMS)} belong to --tier edit")
    if args.hidden_dir and args.tier not in ("v2", "edit"):
        raise UsageError("--hidden-dir adds hidden cases to the v2 or edit tier")
    if not lang_names or len(set(lang_names)) != len(lang_names):
        raise UsageError("--langs needs one or more distinct languages, e.g. nyra,python,typescript,rust")
    if args.repairs < 0 or args.samples < 1 or args.jobs < 1 or args.time_runs < 0:
        raise UsageError("--repairs must be >= 0, --samples >= 1, --jobs >= 1, --time-runs >= 0")
    if args.budget is not None and (args.provider not in ("openrouter", "anthropic") or args.budget <= 0):
        raise UsageError("--budget takes a positive number of dollars and needs --provider openrouter or anthropic "
                         "(the providers that know what each call cost)")
    if args.provider == "anthropic" and args.effort in ("none", "minimal"):
        raise UsageError("--effort none and minimal are OpenRouter reasoning levels; the anthropic provider takes "
                         "low, medium, high, xhigh or max")
    extra = None
    if args.extra_json:
        try:
            extra = json.loads(args.extra_json)
        except ValueError as exc:
            raise UsageError(f"--extra-json is not valid JSON: {exc}") from None
        if not isinstance(extra, dict):
            raise UsageError("--extra-json must be a JSON object")
    model_ids = resolve_models(args)

    py_checker, ts_checker, typechecks, check_notes = resolve_typechecks(args)
    langs = make_languages(lang_names, nyra=args.nyra, backend=args.backend, spec=Path(args.spec),
                           timeout=args.timeout, node=args.node, rustc=args.rustc, time_runs=args.time_runs,
                           python_checker=py_checker, ts_checker=ts_checker)
    warnings: list = []
    for lang in langs.values():
        warnings += lang.preflight()
    warnings += [n for n in check_notes if n.startswith(typecheck.SKIPPED_PREFIX)]
    for w in warnings:
        print(f"warning: {w}", file=sys.stderr)
    for n in check_notes:
        if not n.startswith(typecheck.SKIPPED_PREFIX):
            print(n)

    options = dict(reference=lambda lang, tid: langs[lang].reference_code(tid),
                   flaky=(True if args.mock_flaky == "mix" else (args.mock_flaky or False)),
                   max_tokens=args.max_tokens, effort=args.effort, extra=extra, count_tokens=not args.no_count_tokens)
    if args.base_url:
        options["base_url"] = args.base_url
    try:
        plist = [providers.make_provider(args.provider, m, **options) for m in model_ids]
    except ValueError as exc:
        raise UsageError(str(exc)) from None
    provider = plist[0]
    listing = (check_openrouter_models(model_ids, args.base_url, args.max_tokens)
               if args.provider == "openrouter" and not args.no_model_check else None)

    # Which Nyra version do the tasks have to fit? Default: the compiler we are about to test.
    nyra = nyra_of(langs)
    if args.max_version:
        max_version, max_source = parse_version(args.max_version), "flag"
        if nyra is not None and nyra.version() is not None and max_version > nyra.version():
            warnings.append(f"--max-version {args.max_version} is above the compiler's version {nyra.version_text()}: "
                            "tasks the compiler cannot express will count as Nyra failures")
            print(f"warning: {warnings[-1]}", file=sys.stderr)
    elif nyra is not None and nyra.version() is not None:
        max_version, max_source = nyra.version(), "compiler"
    else:
        max_version, max_source = None, None

    try:
        all_tasks = load_tier(args.tier)
    except ValueError as exc:
        raise UsageError(str(exc)) from None
    hidden_info = None
    if args.hidden_dir:
        all_tasks, hidden_info = apply_hidden_dir(all_tasks, Path(args.hidden_dir))
    tasks, excluded = select_tasks(all_tasks, args.tasks, max_version, lang_names, provider)
    if args.tier == "safety":
        import safety  # the safety tier is a design: its tasks are pending until the compiler has `--allow`
        tasks, pending = safety.gate(tasks, nyra, args.include_pending)
        excluded += pending
        if pending:
            print(f"safety tier: {len(pending)} task(s) are pending and were not run: {safety.PENDING_REASON} "
                  "(--include-pending runs them anyway)")
            if args.dry_run and not tasks:
                for e in pending:
                    print(f"  pending {e['id']}")
                return 0
    if not tasks:
        raise UsageError("no tasks selected" if args.tier != "safety" else
                         "no safety task can run yet (see the message above); --dry-run lists them")

    n_jobs = len(tasks) * len(lang_names) * args.samples
    calls_per_model = n_jobs * (args.repairs + 1)
    banner = (f"Nyra benchmark | provider={provider.name} model={', '.join(model_ids)} | {len(tasks)} tasks x "
              f"{len(lang_names)} languages x {args.samples} sample(s) | up to {args.repairs + 1} attempts each "
              f"(at most {calls_per_model} model calls" + (f" per model, {calls_per_model * len(model_ids)} in total"
                                                           if len(model_ids) > 1 else "") + ")")
    if nyra is not None:
        banner += f" | {nyra.version_text()} ({nyra.backend})"
    if args.tier != "v1":
        banner += f" | tier {args.tier}" + (f", preset {args.preset}" if args.preset else "")
    print(banner)
    if max_version is not None:
        print(f"tasks limited to Nyra <= {max_version[0]}.{max_version[1]} (from {max_source}); "
              f"{len(excluded)} task(s) not run")
    if args.dry_run:
        for t in tasks:
            print(f"  {t.min_version}  {t.id:<24} {t.category:<14} {t.title}")
        for e in excluded:
            print(f"  skipped {e['id']}: {e['reason']}")
        if listing:
            by_id = {m["id"]: m for m in listing}
            total = 0.0
            print(f"estimated cost (about {ASSUMED_ATTEMPTS} attempts per run and {args.assume_output_tokens:,} output "
                  "tokens per attempt, thinking included; thinking models can use several times more):")
            for mid in model_ids:
                price = modelsmod.price_per_token(modelsmod.lookup(by_id, mid) or {})
                if price is None:
                    print(f"  {mid}: price not listed")
                    continue
                cost = estimate_cost(price, tasks, lang_names, langs, args.samples, args.assume_output_tokens)
                total += cost
                print(f"  {mid}: about ${cost:,.2f}")
            if len(model_ids) > 1:
                print(f"  total: about ${total:,.2f}")
            print("  (use --budget USD to stop a run that costs more than you planned)")
        return 0

    for p in plist:
        p.ensure_ready()  # a missing key or SDK stops the run here, before anything is spent
    if args.time_runs:
        for lang in langs.values():
            lang.startup_ms()  # measured now, before the parallel jobs start, so that they cannot slow it down
    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)
    plan = Plan(args=args, lang_names=lang_names, langs=langs, tasks=tasks, excluded=excluded, max_version=max_version,
                max_source=max_source, warnings=warnings, toolchains=toolchain_info(langs), typechecks=typechecks,
                hidden=hidden_info)
    budget = BudgetGuard(args.budget, plist) if args.budget is not None else None

    outcomes: list = []
    not_run = 0
    for i, p in enumerate(plist, 1):
        if len(plist) > 1:
            print(f"\n=== model {i} of {len(plist)}: {p.model} ===", flush=True)
        outcome = run_model(plan, p, out_dir, budget, single=len(plist) == 1)
        outcomes.append(outcome)
        if outcome.interrupted or outcome.stop_all:
            not_run = len(plist) - i
            if not_run:
                print(f"stopping: the remaining {not_run} model(s) were not run", file=sys.stderr)
            break

    finished = [o for o in outcomes if o.results is not None]
    if len(plist) > 1 and finished:
        comparison = report.render_comparison([o.results for o in finished])
        cmp_json, cmp_md = result_paths(out_dir, finished[0].results["run"]["date"], provider.name, "compare")
        cmp_md.write_text(comparison, encoding="utf-8")
        (out_dir / "latest.md").write_text(comparison, encoding="utf-8")  # the comparison is what this run is about
        # An index of the per-model files, so the set can be found again (python bench/publish.py reads them).
        cmp_json.write_text(json.dumps(
            {"schema": SCHEMA_VERSION, "date": finished[0].results["run"]["date"], "provider": provider.name,
             "langs": lang_names, "models": [{"model": o.model, "file": o.json_path.name,
                                             "complete": o.results["run"]["complete"]} for o in finished]},
            indent=2) + "\n", encoding="utf-8")
        print()
        print(comparison)
        print(f"wrote {cmp_md}\nper-model results: " + ", ".join(o.json_path.name for o in finished))
    if budget is not None and finished:
        print(f"spent ${budget.spent():.4f} of the ${budget.limit:g} budget")
    codes = [o.exit_code for o in outcomes] + ([2] if not_run else [])
    for code in (130, 2, 1):  # the most serious problem decides the exit status
        if code in codes:
            return code
    return 0


if __name__ == "__main__":
    sys.exit(main())
