"""`Sandbox`: runs, checks and fixes Nyra code with the `nyra` executable.

Every run goes through `nyra run --sandbox` (the interpreter: no capability but those in `allow`,
file paths confined to the working folder, limits on steps, memory, output, call depth and time)
unless `native=True` is asked for. The program is written to a fresh temporary folder that is
removed when the call returns.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import tempfile
import time
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Any, Dict, List, Mapping, Optional, Sequence

from ._binary import find_binary

CAPABILITIES = ("fs", "input", "os", "net")
FILE = "main.nyra"

# the line `--time` prints after an interpreted run
_TIME_LINE = re.compile(r"^nyra .*\| interpreted ([\d.]+) ms \((\d+) steps")
# runtime errors that mean a limit stopped the program
LIMITS = {"E0355": "fuel", "E0356": "memory", "E0357": "output", "E0358": "depth", "E0359": "time"}


class NyraError(RuntimeError):
    """A program did not compile or failed at run time (see `.result`), or its output was not JSON."""

    def __init__(self, message: str, result: Optional["Result"] = None):
        super().__init__(message)
        self.result = result


def _diag(d: Mapping[str, Any]) -> Dict[str, Any]:
    """An error or warning with the keys every caller can rely on (and the others kept)."""
    out = dict(d)
    out["code"] = d.get("code", "")
    out["message"] = d.get("message", "")
    out["line"] = d.get("line", 0)
    out["col"] = d.get("col", 0)
    out["hint"] = d.get("hint") or ""
    out["fix"] = d.get("fix")
    if "file" in out:
        out["file"] = Path(str(out["file"])).name
    return out


def _render(errors: Sequence[Mapping[str, Any]]) -> str:
    lines = []
    for e in errors:
        kind = "runtime error" if e.get("runtime") else "error"
        lines.append(f"{kind}[{e['code']}] line {e['line']}:{e['col']}: {e['message']}")
        if e.get("hint"):
            lines.append(f"  hint: {e['hint']}")
    return "\n".join(lines)


@dataclass
class Result:
    """What a run did. `errors` are compile or runtime errors: dicts with `code`, `message`,
    `line`, `col`, `hint` and `fix` (the edits of a certain fix, or None); `warnings` include the
    fixes applied in memory (with `applied`)."""

    ok: bool
    stdout: str
    stderr: str
    exit_code: int
    errors: List[Dict[str, Any]] = field(default_factory=list)
    warnings: List[Dict[str, Any]] = field(default_factory=list)
    steps: Optional[int] = None
    ms: float = 0.0
    stage: Optional[str] = None  # "compile", "runtime" or None when it ran to the end
    limit: Optional[str] = None  # "fuel", "memory", "output", "depth" or "time" when a limit stopped it
    backend: str = "native"

    def to_dict(self) -> Dict[str, Any]:
        return asdict(self)

    def feedback(self, max_chars: int = 8000) -> str:
        """A short text for a model: the output, then each error with its line and hint."""
        parts = []
        if self.stage == "compile":
            parts.append("The program did not compile.")
        elif self.limit:
            parts.append(f"The program was stopped by the {self.limit} limit (exit {self.exit_code}).")
        elif not self.ok:
            parts.append(f"The program failed (exit {self.exit_code}).")
        out = self.stdout
        if len(out) > max_chars:
            out = out[:max_chars] + f"\n... ({len(self.stdout) - max_chars} more characters)"
        if out or self.ok:
            parts.append("stdout:\n" + out if out else "stdout: (empty)")
        if self.errors:
            parts.append("errors:\n" + _render(self.errors))
        fixed = [w for w in self.warnings if w.get("applied")]
        if fixed:
            parts.append(
                "repaired in memory (write it this way next time):\n"
                + "\n".join(f"  line {w['line']}: {w['message']} -> {w['applied']}" for w in fixed)
            )
        return "\n".join(parts)


@dataclass
class CheckResult:
    ok: bool
    errors: List[Dict[str, Any]] = field(default_factory=list)
    warnings: List[Dict[str, Any]] = field(default_factory=list)
    ms: float = 0.0

    def to_dict(self) -> Dict[str, Any]:
        return asdict(self)


class Sandbox:
    """Runs Nyra programs safely with the `nyra` executable.

    fuel        steps a run may take (E0355 when it runs out)
    timeout     seconds of real time (E0359); the process is also killed a little after it
    memory_mb   heap a run may use (E0356)
    max_output  bytes it may print (E0357)
    max_depth   nested calls (E0358); None: the interpreter's default
    allow       capabilities to grant besides `input`: "fs", "os", "net" (or "all")
    cwd         the working folder; with `allow=["fs"]` the program reads and writes only below it.
                None: a fresh empty temporary folder per run
    strict      do not repair errors that have one certain fix (they are repaired in memory and
                reported as warnings by default, as `nyra run` does)
    native      compile with the C compiler and run natively: NO sandbox (no limits but `timeout`,
                no file confinement); only for code you trust
    binary      the `nyra` executable (default: NYRA_PATH, PATH, then `nyra.install()`'s cache)
    """

    def __init__(
        self,
        fuel: int = 100_000_000,
        timeout: float = 10.0,
        memory_mb: int = 256,
        max_output: int = 1 << 20,
        max_depth: Optional[int] = None,
        allow: Sequence[str] = (),
        cwd: Optional[str] = None,
        strict: bool = False,
        native: bool = False,
        binary: Optional[str] = None,
    ):
        if isinstance(allow, str):
            allow = [a for a in re.split(r"[,\s]+", allow) if a]
        for cap in allow:
            if cap != "all" and cap not in CAPABILITIES:
                raise ValueError(f"unknown capability {cap!r}: use {', '.join(CAPABILITIES)} or 'all'")
        for name, value in (("fuel", fuel), ("memory_mb", memory_mb), ("max_output", max_output)):
            if int(value) < 1:
                raise ValueError(f"{name} must be at least 1")
        if timeout <= 0:
            raise ValueError("timeout must be positive")
        self.fuel = int(fuel)
        self.timeout = float(timeout)
        self.memory_mb = int(memory_mb)
        self.max_output = int(max_output)
        self.max_depth = max_depth
        self.allow = list(allow)
        self.cwd = str(cwd) if cwd is not None else None
        self.strict = strict
        self.native = native
        self.binary = find_binary(binary)

    # ---- helpers ---------------------------------------------------------------------------------

    def _caps(self) -> str:
        if "all" in self.allow:
            return "all"
        return ",".join(sorted({"input", *self.allow}))

    def _exec(self, args: List[str], stdin: str, cwd: str, timeout: float) -> subprocess.CompletedProcess:
        return subprocess.run(
            [self.binary, *args],
            input=stdin.encode("utf-8"),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            cwd=cwd,
            timeout=timeout,
            env={**os.environ, "NO_COLOR": "1"},
        )

    @staticmethod
    def _write(tmp: str, code: str, files: Optional[Mapping[str, str]]) -> str:
        main = os.path.join(tmp, FILE)
        for name, text in (files or {}).items():
            p = Path(tmp, name).resolve()
            if Path(tmp).resolve() not in p.parents or name == FILE:
                raise ValueError(f"file name {name!r} must be a relative path inside the program's folder, not {FILE}")
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(text, encoding="utf-8", newline="\n")
        Path(main).write_text(code, encoding="utf-8", newline="\n")
        return main

    # ---- the API ---------------------------------------------------------------------------------

    def run(
        self,
        code: str,
        stdin: str = "",
        *,
        args: Optional[Sequence[str]] = None,
        files: Optional[Mapping[str, str]] = None,
    ) -> Result:
        """Compile and run `code` with `stdin` as its standard input. `args` go to the program
        (`os.args()`, needs `allow=["os"]`); `files` are more modules next to it (`use ./name`)."""
        start = time.perf_counter()
        with tempfile.TemporaryDirectory(prefix="nyra-") as tmp:
            main = self._write(tmp, code, files)
            cwd = self.cwd or tmp
            src = FILE if cwd == tmp else main
            cmd = ["run", src, "--json", "--allow", self._caps()]
            if self.native:
                cmd.append("--native")
            else:
                cmd += [
                    "--sandbox",
                    "--time",
                    "--fuel",
                    str(self.fuel),
                    "--max-memory",
                    f"{max(1, self.memory_mb)}M",
                    "--max-output",
                    str(self.max_output),
                    "--max-time",
                    str(max(1, int(self.timeout * 1000))),
                ]
                if self.max_depth is not None:
                    cmd += ["--max-depth", str(int(self.max_depth))]
            if self.strict:
                cmd.append("--strict")
            if args:
                cmd += ["--", *[str(a) for a in args]]
            # the sandbox stops itself at `timeout`; the process gets a margin for compiling (and,
            # natively, the C compiler) before it is killed
            hard = self.timeout + (30.0 if self.native else 5.0)
            try:
                p = self._exec(cmd, stdin, cwd, hard)
            except subprocess.TimeoutExpired as e:
                err = {
                    "code": "E0359",
                    "message": f"time limit reached: the program was killed after {hard:g} s",
                    "line": 0,
                    "col": 0,
                    "hint": "an endless loop? Otherwise raise `timeout`",
                    "fix": None,
                    "runtime": True,
                }
                out = (e.stdout or b"").decode("utf-8", "replace")
                return Result(False, out, _render([err]), 124, [err], [], None, _ms(start), "runtime", "time", self._backend())
        return self._parse(p, start)

    def _backend(self) -> str:
        return "native-compiled" if self.native else "native"

    def _parse(self, p: subprocess.CompletedProcess, start: float) -> Result:
        stdout = p.stdout.decode("utf-8", "replace")
        errors: List[Dict[str, Any]] = []
        warnings: List[Dict[str, Any]] = []
        steps = None
        rest = []
        for line in p.stderr.decode("utf-8", "replace").splitlines():
            m = _TIME_LINE.match(line)
            if m:
                steps = int(m.group(2))
                continue
            if line.startswith("{"):
                try:
                    j = json.loads(line)
                except ValueError:
                    j = None
                if isinstance(j, dict) and isinstance(j.get("errors"), list):
                    errors += [_diag(e) for e in j["errors"]]
                    continue
                if isinstance(j, dict) and isinstance(j.get("warnings"), list):
                    warnings += [_diag(w) for w in j["warnings"]]
                    continue
            rest.append(line)
        stage = None
        # a compile error: the errors as JSON on standard output, exit 1, and nothing ran
        if p.returncode == 1 and steps is None and not errors and stdout.startswith('{"ok":false'):
            try:
                j = json.loads(stdout)
                errors = [_diag(e) for e in j.get("errors", [])]
                warnings += [_diag(w) for w in j.get("warnings", [])]
                stdout = ""
                stage = "compile"
            except ValueError:
                pass
        if errors and stage is None:
            stage = "runtime"
        limit = next((LIMITS[e["code"]] for e in errors if e["code"] in LIMITS), None)
        stderr = "\n".join(x for x in [_render(errors), *rest] if x)
        return Result(
            ok=p.returncode == 0 and not errors,
            stdout=stdout,
            stderr=stderr,
            exit_code=p.returncode,
            errors=errors,
            warnings=warnings,
            steps=steps,
            ms=_ms(start),
            stage=stage,
            limit=limit,
            backend=self._backend(),
        )

    def check(self, code: str, *, strict: bool = True, files: Optional[Mapping[str, str]] = None) -> CheckResult:
        """Errors and warnings without running (the `ex` examples run). `strict=False` repairs
        the errors that have one certain fix in memory and lists them as warnings."""
        start = time.perf_counter()
        with tempfile.TemporaryDirectory(prefix="nyra-") as tmp:
            self._write(tmp, code, files)
            cmd = ["check", FILE, "--json", "--allow", self._caps()]
            if strict:
                cmd.append("--strict")
            p = self._exec(cmd, "", tmp, self.timeout + 30)
        try:
            j = json.loads(p.stdout.decode("utf-8", "replace"))
        except ValueError as e:
            raise NyraError(f"nyra check printed no JSON: {p.stderr.decode('utf-8', 'replace').strip()}") from e
        return CheckResult(
            ok=bool(j.get("ok")),
            errors=[_diag(e) for e in j.get("errors", [])],
            warnings=[_diag(w) for w in j.get("warnings", [])],
            ms=_ms(start),
        )

    def fix(self, code: str) -> str:
        """`code` with the certain fixes of its errors applied (`nyra check --fix`). When errors
        without a fix remain, the code comes back unchanged; `check()` lists them."""
        with tempfile.TemporaryDirectory(prefix="nyra-") as tmp:
            main = self._write(tmp, code, None)
            self._exec(["check", FILE, "--fix", "--json", "--allow", self._caps()], "", tmp, self.timeout + 30)
            return Path(main).read_text(encoding="utf-8")

    def explain(self, code: str) -> Dict[str, Any]:
        """The entry of an error code (`nyra explain E0201 --json`) with its text for people in
        `text`. An unknown code gives `{"ok": false, "error": ..., "did_you_mean": ...}`."""
        with tempfile.TemporaryDirectory(prefix="nyra-") as tmp:
            p = self._exec(["explain", code, "--json"], "", tmp, 30)
            j = json.loads(p.stdout.decode("utf-8", "replace"))
            if p.returncode == 0 and isinstance(j, dict) and "code" in j:
                text = self._exec(["explain", code], "", tmp, 30)
                j.setdefault("ok", True)
                j["text"] = text.stdout.decode("utf-8", "replace")
            return j

    def run_json(self, code: str, stdin: str = "", **kwargs: Any) -> Any:
        """Run `code` and parse what it printed as one JSON value (`print(json.str(value))`, or a
        `main` that returns a value). Raises `NyraError` when it fails or prints no JSON."""
        r = self.run(code, stdin, **kwargs)
        if not r.ok:
            raise NyraError(r.feedback(), r)
        text = r.stdout.strip()
        try:
            return json.loads(text)
        except ValueError:
            pass
        last = text.splitlines()[-1] if text else ""
        try:
            return json.loads(last)
        except ValueError as e:
            raise NyraError(f"the program printed no JSON value: {text[:200]!r}", r) from e

    def version(self) -> str:
        p = subprocess.run([self.binary, "--version"], stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=30)
        return (p.stdout or p.stderr).decode().strip().split()[-1]


def _ms(start: float) -> float:
    return round((time.perf_counter() - start) * 1000, 1)
