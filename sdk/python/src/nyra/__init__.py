"""Run AI-written Nyra code safely from Python.

    from nyra import Sandbox
    box = Sandbox(fuel=10_000_000, timeout=5)
    r = box.run('print("hi")')
    r.ok, r.stdout, r.errors

Needs the `nyra` executable: on PATH, in NYRA_PATH, or downloaded by `nyra.install()`.
"""

from __future__ import annotations

from typing import Any, Dict

from ._binary import InstallError, NyraNotFound, cache_dir, find_binary, install, latest_version
from ._sandbox import CAPABILITIES, CheckResult, NyraError, Result, Sandbox

__version__ = "0.9.0"


def run(code: str, stdin: str = "", **sandbox: Any) -> Result:
    """`Sandbox(**sandbox).run(code, stdin)`."""
    return Sandbox(**sandbox).run(code, stdin)


def check(code: str, **sandbox: Any) -> CheckResult:
    return Sandbox(**sandbox).check(code)


def fix(code: str, **sandbox: Any) -> str:
    return Sandbox(**sandbox).fix(code)


def explain(code: str, **sandbox: Any) -> Dict[str, Any]:
    return Sandbox(**sandbox).explain(code)


def run_json(code: str, stdin: str = "", **sandbox: Any) -> Any:
    return Sandbox(**sandbox).run_json(code, stdin)


__all__ = [
    "Sandbox",
    "Result",
    "CheckResult",
    "NyraError",
    "NyraNotFound",
    "InstallError",
    "CAPABILITIES",
    "run",
    "check",
    "fix",
    "explain",
    "run_json",
    "install",
    "find_binary",
    "cache_dir",
    "latest_version",
    "__version__",
]
