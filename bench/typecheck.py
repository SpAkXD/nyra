"""Optional static type checks for the Python and TypeScript arms (`--python-typecheck`, `--ts-typecheck`).

Nyra's compiler type-checks every program before it runs and returns structured errors. Python and TypeScript, as the
benchmark runs them, do not: Python's interpreter ignores annotations and Node.js strips TypeScript's types without
looking at them. Without a checker those arms are easier than Nyra on exactly the mistakes a type checker catches. With
these flags the harness runs a real checker first (mypy or pyright for Python, tsc for TypeScript) and counts a program
the checker rejects as a compile error, like a Nyra program `nyra check` rejects, with the checker's messages as the
repair feedback. The model is told in its system prompt that its program is checked.

Nothing is installed here: a checker that is not on this machine is reported as unavailable and the arm stays unchecked,
with a clear message (and `typecheck` in the result file says so), so a result never claims a check that did not run.

Standard library only. The functions that look for tools take `which` and `probe` so tests can fake a machine.
"""

from __future__ import annotations

import dataclasses
import os
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Callable, Optional

# Python: Nyra requires fully typed function signatures, so the Python arm is checked the same way (an unannotated
# function is an error), with the bodies checked too. Third-party imports cannot occur (standard library only).
MYPY_FLAGS = ("--disallow-untyped-defs", "--disallow-incomplete-defs", "--check-untyped-defs", "--no-implicit-optional",
              "--ignore-missing-imports", "--no-error-summary", "--no-color-output", "--hide-error-context",
              "--cache-dir", os.devnull)
# TypeScript: strict, without emitting, against the ES2022 library only. The programs use `require`, `process` and
# `console` (Node.js's own), which the standard lib alone does not declare, and no @types/node is installed, so a small
# declaration file stands in for them (everything it declares is `any`).
TSC_FLAGS = ("--noEmit", "--strict", "--target", "es2022", "--lib", "es2022", "--pretty", "false", "--skipLibCheck")
TS_SHIM_NAME = "node_shim.d.ts"
TS_SHIM = """\
declare const process: any;
declare const console: { log(...args: any[]): void; error(...args: any[]): void; warn(...args: any[]): void };
declare function require(name: string): any;
declare const module: any;
declare const Buffer: any;
declare module "fs";
declare module "node:fs";
declare module "readline";
declare module "node:readline";
declare module "os";
declare module "path";
declare module "util";
declare module "assert";
"""

SKIPPED_PREFIX = "skipped"


@dataclasses.dataclass(frozen=True)
class Checker:
    """A type checker that works on this machine."""
    name: str  # mypy | pyright | tsc
    argv: tuple  # the command, without the file to check
    version: str  # e.g. "mypy 1.11.2"
    shim: Optional[str] = None  # a declaration file to write next to the program (TypeScript)

    @property
    def display(self) -> str:
        return {"mypy": "mypy", "pyright": "pyright", "tsc": "The TypeScript compiler (`tsc`)"}.get(self.name, self.name)

    def command(self, filename: str) -> list:
        extra = [TS_SHIM_NAME] if self.shim else []
        return [*self.argv, filename, *extra]

    def describe(self) -> dict:
        return {"tool": self.name, "version": self.version, "status": "checked"}


Probe = Callable[[list], Optional[str]]


def default_probe(argv: list) -> Optional[str]:
    """Run `argv`; its first output line when it exits 0, else None (not installed, or broken)."""
    try:
        p = subprocess.run([str(a) for a in argv], capture_output=True, text=True, timeout=60)
    except (OSError, subprocess.TimeoutExpired):
        return None
    if p.returncode != 0:
        return None
    text = (p.stdout or p.stderr).strip()
    return text.splitlines()[0] if text else ""


def find_python_checker(choice: str = "auto", which: Optional[Callable] = None, probe: Optional[Probe] = None,
                        python: str = sys.executable) -> tuple:
    """(Checker or None, message). `choice`: auto (mypy, then pyright), mypy or pyright."""
    which, probe = which or shutil.which, probe or default_probe
    if choice not in ("auto", "mypy", "pyright"):
        raise ValueError(f"--python-typecheck: use auto, mypy or pyright, not {choice!r}")
    if choice in ("auto", "mypy"):
        version = probe([python, "-m", "mypy", "--version"])
        if version is not None:
            return Checker("mypy", (python, "-m", "mypy", *MYPY_FLAGS), version or "mypy"), f"Python is checked with {version}"
    if choice in ("auto", "pyright"):
        exe = which("pyright")
        version = probe([exe, "--version"]) if exe else None
        if version is not None and exe:
            return Checker("pyright", (exe,), f"pyright {version}".replace("pyright pyright", "pyright")), \
                f"Python is checked with pyright {version}"
    wanted = "mypy or pyright" if choice == "auto" else choice
    return None, (f"{SKIPPED_PREFIX}: --python-typecheck needs {wanted}, which is not installed (pip install mypy); "
                  "the Python programs are NOT type-checked in this run")


def find_ts_checker(choice: str = "auto", which: Optional[Callable] = None, probe: Optional[Probe] = None) -> tuple:
    """(Checker or None, message). Looks for `tsc` on PATH (npm install -g typescript)."""
    which, probe = which or shutil.which, probe or default_probe
    if choice not in ("auto", "tsc"):
        raise ValueError(f"--ts-typecheck: use auto or tsc, not {choice!r}")
    exe = which("tsc")
    version = probe([exe, "--version"]) if exe else None
    if version is not None and exe:
        return Checker("tsc", (exe, *TSC_FLAGS), version or "tsc", shim=TS_SHIM), f"TypeScript is checked with {version}"
    return None, (f"{SKIPPED_PREFIX}: --ts-typecheck needs tsc, which is not installed (npm install -g typescript); "
                  "the TypeScript programs are NOT type-checked in this run")


def system_note(language: str, checker: Checker) -> str:
    """The sentence added to the system prompt so that the model knows its program is checked."""
    if language == "python":
        extra = (" Every function must have complete type annotations (parameters and return type)."
                 if checker.name == "mypy" else "")
        return (f"The program is also type-checked with {checker.name} before it is run, and a program the checker "
                f"rejects counts as a failed attempt.{extra}")
    return ("The program is also type-checked with the TypeScript compiler in strict mode before it is run (the Node.js "
            "globals `require`, `process` and `console` are available as untyped), and a program the compiler rejects "
            "counts as a failed attempt.")


def metadata(language: str, requested: Optional[str], checker: Optional[Checker], message: str) -> dict:
    """What the result file records about the check of one language."""
    if requested is None:
        return {"requested": False, "status": "not requested: the programs are not type-checked"}
    if checker is None:
        return {"requested": True, "status": message}
    return {"requested": True, **checker.describe()}


def clean_output(text: str, workdir: Path) -> str:
    """Remove the scratch directory from a checker's messages (they name `main.py:3: error: ...`)."""
    for variant in sorted({str(workdir), workdir.as_posix(), str(workdir.resolve()), workdir.resolve().as_posix()},
                          key=len, reverse=True):
        for sep in ("\\", "/"):
            text = text.replace(variant + sep, "")
        text = text.replace(variant, ".")
    return text.strip()
