"""The edit tier: change an existing program.

A task of the edit tier gives the model a program of 150 to 400 lines and a change request. How the model returns the
change is what is compared, so each "language" of an edit run is an **arm**:

    nyra-edit       Nyra with symbol-level editing: the model replies with an edit script for `nyra edit`
                    (`@replace NAME` followed by the whole new function, `@add`, `@delete`, `@rename`, ...), and the
                    compiler applies it to the program and refuses a result that does not compile
    python-rewrite  Python: the model replies with the complete modified program
    python-diff     Python: the model replies with a unified diff (the format `diff -u` prints)

Every arm sees the whole program in the prompt (the same program, written in the arm's language), and every arm is judged
the same way: the program that comes out of the edit is run on the example and on hidden inputs, exactly like a v2
task, and it passes only if it is right on all of them. The measure of the tier is the **output tokens per successful
edit**: what the model had to write, billed, for each change that worked (see tier_stats.py).

Files: bench/tasks/edit/<id>.json (the change request and the cases), <id>.base.py and <id>.base.nyra (the programs
the edit starts from), and the reference edits in bench/solutions/edit/<arm>/<id>.{py,diff,edit} (the mock provider
replays them and verify.py checks them).

Standard library only. This module imports `run`; `run.make_languages` imports it lazily.
"""

from __future__ import annotations

import re
from pathlib import Path
from typing import Optional

import run

ARMS = ("nyra-edit", "python-rewrite", "python-diff")
EDIT_REFERENCES = run.SOLUTIONS_DIR / "edit"


class PatchError(Exception):
    """A unified diff that cannot be applied to the program."""


# ----------------------------------------------------------------------------------------- unified diffs

_HUNK = re.compile(r"^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@")


def parse_hunks(diff: str) -> list:
    """The hunks of a unified diff as [(expected start line, [("context"|"remove"|"add", text), ...])].

    Tolerant of what language models actually write: file headers and `diff`/`index` lines are skipped, the numbers in
    the `@@` lines are only a hint (they are often wrong), a context line that lost its leading space is accepted, and
    `\\ No newline at end of file` is ignored."""
    hunks: list = []
    current: Optional[list] = None
    for line in diff.replace("\r\n", "\n").split("\n"):
        m = _HUNK.match(line)
        if m:
            current = []
            hunks.append((int(m.group(1)), current))
            continue
        if current is None:
            continue  # file header, `diff --git`, `index`, prose
        if line.startswith("diff --git"):  # the start of another file's diff: only one file is edited
            break
        if line.startswith("\\"):
            continue
        if line.startswith("+"):
            current.append(("add", line[1:]))
        elif line.startswith("-"):
            current.append(("remove", line[1:]))
        elif line.startswith(" "):
            current.append(("context", line[1:]))
        elif line == "":
            current.append(("context", ""))
        else:
            current.append(("context", line))  # a context line whose leading space was dropped
    # an empty line at the very end of the diff text is the end of the text, not a blank context line
    for _, body in hunks:
        while body and body[-1] == ("context", ""):
            body.pop()
    return [h for h in hunks if h[1]]


def _find(lines: list, old: list, hint: int, loose: bool) -> Optional[int]:
    """Index in `lines` where the block `old` starts (nearest to `hint`), comparing exactly or, `loose`, without
    trailing whitespace. None when it is nowhere."""
    if not old:
        return min(max(hint, 0), len(lines))
    norm = (lambda s: s.rstrip()) if loose else (lambda s: s)
    target = [norm(x) for x in old]
    best = None
    for i in range(0, len(lines) - len(old) + 1):
        if [norm(x) for x in lines[i:i + len(old)]] == target:
            if best is None or abs(i - hint) < abs(best - hint):
                best = i
    return best


def apply_unified_diff(base: str, diff: str) -> str:
    """The program after the diff. Raises PatchError(message) when a hunk does not fit."""
    hunks = parse_hunks(diff)
    if not hunks:
        raise PatchError("no hunk found: the diff needs lines starting with `@@ -start,count +start,count @@`")
    lines = base.replace("\r\n", "\n").split("\n")
    trailing_newline = lines and lines[-1] == ""
    if trailing_newline:
        lines.pop()
    shift = 0
    floor = 0
    for number, (start, body) in enumerate(hunks, 1):
        old = [t for kind, t in body if kind != "add"]
        new = [t for kind, t in body if kind != "remove"]
        hint = max(start - 1 + shift, floor)
        at = _find(lines, old, hint, loose=False)
        if at is None:
            at = _find(lines, old, hint, loose=True)
        if at is None:
            first = next((t for t in old if t.strip()), old[0] if old else "")
            raise PatchError(f"hunk {number} does not apply: the lines it expects (starting with `{first.strip()[:60]}`) "
                             "are not in the program, or not in that order")
        if at < floor:
            raise PatchError(f"hunk {number} overlaps the hunk before it; hunks must be in file order")
        lines[at:at + len(old)] = new
        shift += len(new) - len(old)
        floor = at + len(new)
    return "\n".join(lines) + ("\n" if trailing_newline else "")


def make_unified_diff(base: str, new: str, name: str = "main") -> str:
    """The diff `diff -u` would print (3 lines of context): used to generate the reference diffs."""
    import difflib
    return "".join(difflib.unified_diff(base.splitlines(keepends=True), new.splitlines(keepends=True),
                                        f"a/{name}", f"b/{name}", n=3))


# ------------------------------------------------------------------------------------------------- arms

_EDIT_TASK = ("You are given a complete program and a change request. The program reads its input from standard input and "
              "prints its result to standard output. After your change it is run on several inputs, of which you see "
              "only one example, and for each input its standard output must be exactly what the change request "
              "describes; everything the request does not mention must keep working as before.")

_PYTHON_REWRITE = ("You change Python 3 programs, using only the standard library. " + _EDIT_TASK + " Reply with exactly "
                   "one fenced code block that contains the complete modified program, and no other text.")

_PYTHON_DIFF = ("You change Python 3 programs, using only the standard library. " + _EDIT_TASK + " Reply with exactly one "
                "fenced code block that contains a unified diff in the format `diff -u` prints, that turns the given "
                "program (the file `main.py`) into the modified program: a `--- a/main.py` and a `+++ b/main.py` line, "
                "then hunks that start with `@@ -start,count +start,count @@`, in which unchanged lines start with a "
                "space, removed lines with `-` and added lines with `+`. Reply with the diff and no other text.")

_NYRA_EDIT = """\
You write programs in Nyra, a new programming language that you have not seen before. The complete language \
specification is below. It is the only documentation you have.

<nyra_spec>
{spec}
</nyra_spec>

""" + _EDIT_TASK + """

You change the program with an edit script for the tool `nyra edit`, which replaces whole functions and structs by \
name and leaves the rest of the file untouched. An edit script is one or more commands, each followed by its code on \
the next lines:

@replace NAME                 the next lines are the complete new definition of the function or struct NAME \
(`Struct.field` replaces one field with one line `name: type`)
@add                          the next lines are new definitions, added at the end of the file
@add after NAME               ... or right after the definition NAME (`@add before NAME` puts them before it)
@delete NAME                  removes a function, a struct or a field
@rename NAME NEW              renames it and every reference to it
@add-field STRUCT name: type  adds a field to a struct (`... after FIELD` chooses the place)

Only the functions and structs you name change. The program must still compile after the edit: an edit that adds \
errors is refused and nothing changes. Reply with exactly one fenced code block that contains the edit script, and no \
other text."""

_REPLY_REWRITE = ("Reply with exactly one fenced code block that contains the complete modified program, "
                  "and no other text.")
_REPLY_DIFF = ("Reply with exactly one fenced code block that contains the corrected unified diff, written against the "
               "ORIGINAL program (not against your earlier diff), and no other text.")
_REPLY_EDIT = ("Reply with exactly one fenced code block that contains the corrected edit script, written against the "
               "ORIGINAL program (the earlier edit was not kept), and no other text.")


class EditMixin:
    """What the three arms share: where the base program is, and the prompt."""

    base_ext = ""  # the extension of the program the edit starts from
    reply_rule = ""
    _plain = False  # True during the toolchain self-test, which runs a whole program, not an edit

    def preflight(self) -> list:
        self._plain = True
        try:
            return super().preflight()
        finally:
            self._plain = False

    def base_program(self, task: run.Task) -> str:
        path = task.path.parent / f"{task.id}.base{self.base_ext}"
        if not path.is_file():
            raise run.HarnessError(f"edit task {task.id} has no {path.name}")
        return path.read_text(encoding="utf-8")

    def reference_path(self, task_id: str) -> Path:
        return EDIT_REFERENCES / self.name / f"{task_id}{self.ext}"

    def system_prompt_for(self, task: run.Task) -> str:
        return self.system_prompt

    def prompt_for(self, task: run.Task) -> str:
        label = "Nyra" if self.base_ext == ".nyra" else "Python"
        program = self.base_program(task)
        if not program.endswith("\n"):
            program += "\n"
        return f"Here is a {label} program:\n\n<program>\n{program}</program>\n\nChange request: " + run.task_prompt(task)

    def no_code_feedback(self) -> str:
        return ("Your reply did not contain a fenced code block, so there was nothing to apply. " + self.reply_rule)

    def tidy(self, result: run.EvalResult) -> run.EvalResult:
        """The generic feedback ends with 'Fix the program. Reply with the complete corrected program': say what this
        arm is to reply with instead."""
        result.feedback = (result.feedback.replace(run._FIX, "Fix it. " + self.reply_rule)
                           .replace(run._REPLY_RULE, self.reply_rule))
        return result

    def rejected(self, tool: str, detail: str) -> run.EvalResult:
        return run.EvalResult(False, "compile_error", stderr=detail,
                              feedback=(f"{tool} could not apply your change:\n\n<tool_output>\n"
                                        f"{run.clip_head(detail, 30, 3000)}\n</tool_output>\n\n{self.reply_rule}"))


class PythonRewriteArm(EditMixin, run.PythonLang):
    name = "python-rewrite"
    display = "Python (rewrite)"
    ext = ".py"
    base_ext = ".py"
    reply_rule = _REPLY_REWRITE

    @property
    def system_prompt(self) -> str:
        return _PYTHON_REWRITE + self.type_note()

    def evaluate(self, code: str, task: run.Task, timed: bool = True) -> run.EvalResult:
        if self._plain:
            return super().evaluate(code, task, False)
        return self.tidy(super().evaluate(code, task, False))


class PythonDiffArm(EditMixin, run.PythonLang):
    name = "python-diff"
    display = "Python (diff)"
    ext = ".diff"
    base_ext = ".py"
    reply_rule = _REPLY_DIFF

    @property
    def system_prompt(self) -> str:
        return _PYTHON_DIFF + self.type_note()

    def evaluate(self, code: str, task: run.Task, timed: bool = True) -> run.EvalResult:
        if self._plain:
            return super().evaluate(code, task, False)
        try:
            program = apply_unified_diff(self.base_program(task), code)
        except PatchError as exc:
            return self.rejected("The patch tool", str(exc))
        return self.tidy(super().evaluate(program, task, False))


class NyraEditArm(EditMixin, run.NyraLang):
    name = "nyra-edit"
    display = "Nyra (edit)"
    ext = ".edit"
    base_ext = ".nyra"
    reply_rule = _REPLY_EDIT

    @property
    def system_prompt(self) -> str:
        return _NYRA_EDIT.format(spec=self.spec.strip())

    def apply_edit(self, script: str, task: run.Task) -> tuple:
        """Run `nyra edit` on the base program in a scratch folder. Returns (program text or None, the tool's message)."""
        with run.scratch_dir() as wd:
            env = run.child_env(wd)
            run.write_source(wd / "main.nyra", self.base_program(task))
            proc = run.run_limited([self.bin, "edit", "main.nyra"], cwd=wd, env=env, timeout=run.CHECK_TIMEOUT,
                                   stdin=(script.rstrip("\n") + "\n").encode("utf-8"))
            if proc.spawn_error:
                raise run.HarnessError(f"cannot run the Nyra compiler {self.bin}: {proc.spawn_error}")
            message = run.scrub_paths((proc.stderr + proc.stdout).decode("utf-8", "replace"), wd).strip()
            if proc.returncode != 0:
                return None, message or f"nyra edit exited with code {proc.returncode}"
            return (wd / "main.nyra").read_text(encoding="utf-8"), message

    def evaluate(self, code: str, task: run.Task, timed: bool = True) -> run.EvalResult:
        if self._plain:
            return super().evaluate(code, task, False)
        program, message = self.apply_edit(code, task)
        if program is None:
            return self.rejected("`nyra edit`", message)
        return self.tidy(super().evaluate(program, task, False))

    def self_repair(self, code: str, task: run.Task) -> dict:
        return {"tried": False}  # `nyra check --fix` repairs programs, not edit scripts


ARM_CLASSES = {"nyra-edit": NyraEditArm, "python-rewrite": PythonRewriteArm, "python-diff": PythonDiffArm}
