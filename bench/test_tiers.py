"""Tests of the benchmark tiers beyond v1: hidden inputs (v2), edits, safety, type checks, presets and the leaderboard.

These tests are imported by test_bench.py (the one file CI runs), so `python bench/test_bench.py` and
`python -m unittest bench.test_bench` run them. No network, no API key; the ones that need the compiler skip without it.
"""

from __future__ import annotations

import contextlib
import dataclasses
import io
import json
import re
import shutil
import sys
import tempfile
import unittest
import unittest.mock
from pathlib import Path
from types import SimpleNamespace

BENCH_DIR = Path(__file__).resolve().parent
if str(BENCH_DIR) not in sys.path:
    sys.path.insert(0, str(BENCH_DIR))

import edit_arms  # noqa: E402
import leaderboard  # noqa: E402
import models as modelsmod  # noqa: E402
import providers  # noqa: E402
import publish  # noqa: E402
import report  # noqa: E402
import run  # noqa: E402
import safety  # noqa: E402
import tier_stats  # noqa: E402
import typecheck  # noqa: E402
import verify  # noqa: E402
import verify_tiers  # noqa: E402

try:
    NYRA = run.find_nyra()
except run.HarnessError:
    NYRA = None
needs_nyra = unittest.skipUnless(NYRA, "the nyra compiler is not built (cargo build --release)")


def cases_task(cases, tier="v2", prompt="Read numbers and print their sum.", raw=None):
    return run.Task("t", "T", prompt, cases[0][1], "0.5", "x", "", Path("."), tier=tier, raw=raw,
                    cases=tuple(run.Case(stdin=s, expected_output=o, visible=(i == 0), name="example" if i == 0 else f"hidden{i}")
                                for i, (s, o) in enumerate(cases)))


SUM_CASES = [("1\n2\n", "3\n"), ("10\n20\n30\n", "60\n"), ("5\n", "5\n"), ("", "0\n")]
SUM_PROGRAM = "import sys\nprint(sum(int(x) for x in sys.stdin.read().split()))\n"


def run_cli(*argv):
    out, err = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        code = run.main(list(argv))
    return code, out.getvalue(), err.getvalue()


# ---------------------------------------------------------------------------------------- the case format


class CaseFormat(unittest.TestCase):
    def write(self, directory, name, data):
        (Path(directory) / f"{name}.json").write_text(json.dumps(data), encoding="utf-8")

    def task_json(self, **over):
        data = {"id": "t1", "title": "T", "category": "x", "min_version": "0.5", "prompt": "Sum.",
                "cases": [{"visible": True, "stdin": "1\n", "expected_output": "1\n"},
                          {"stdin": "2\n", "expected_output": "2\n"}, {"stdin": "3\n", "expected_output": "3\n"}]}
        data.update(over)
        return data

    def load(self, data, tier="v2", require_expected=True):
        with tempfile.TemporaryDirectory() as tmp:
            self.write(tmp, data["id"], data)
            return run.load_tasks(Path(tmp), require_expected, tier=tier)

    def test_a_task_with_cases_has_an_example_and_hidden_cases(self):
        (t,) = self.load(self.task_json())
        self.assertEqual(t.example.stdin, "1\n")
        self.assertEqual([c.stdin for c in t.hidden_cases], ["2\n", "3\n"])
        self.assertEqual(t.expected_output, "1\n")  # the example's output: v1 code paths keep working
        self.assertEqual(t.tier, "v2")

    def test_exactly_one_visible_case_and_at_least_two_hidden_ones(self):
        two_visible = self.task_json()
        two_visible["cases"][1]["visible"] = True
        for data, message in ((two_visible, "exactly one case must be `visible`"),
                              (self.task_json(cases=self.task_json()["cases"][:2]), "at least two hidden"),
                              (self.task_json(cases=[]), "non-empty list"),
                              (self.task_json(cases=[{"visible": True, "expected_output": "1\n"}] * 3), "string `stdin`")):
            with self.assertRaisesRegex(ValueError, message):
                self.load(data)

    def test_an_empty_expected_output_is_refused_unless_it_is_being_generated(self):
        data = self.task_json()
        data["cases"][2]["expected_output"] = ""
        with self.assertRaisesRegex(ValueError, "empty expected_output"):
            self.load(data)
        self.assertEqual(len(self.load(data, require_expected=False)[0].cases), 3)

    def test_cases_belong_to_the_v2_tier(self):
        with self.assertRaisesRegex(ValueError, "v2 tier"):
            self.load(self.task_json(), tier="v1")

    def test_case_names_must_differ(self):
        data = self.task_json()
        data["cases"][1]["name"] = data["cases"][2]["name"] = "same"
        with self.assertRaisesRegex(ValueError, "unique"):
            self.load(data)

    def test_the_prompt_shows_the_example_and_nothing_else(self):
        task = cases_task([("1\n2\n", "3\n"), ("HIDDEN-IN-9\n", "HIDDEN-OUT-9\n"), ("x\n", "y\n")])
        text = run.task_prompt(task)
        self.assertIn("<example_input>\n1\n2\n</example_input>", text)
        self.assertIn("<example_output>\n3\n</example_output>", text)
        self.assertNotIn("HIDDEN", text)
        self.assertEqual(run.task_prompt(dataclasses.replace(task, cases=())), task.prompt)  # no cases: the prompt as it is

    def test_the_system_prompt_for_a_stdin_task_says_that_there_are_hidden_inputs(self):
        lang = run.PythonLang()
        v1 = lang.system_prompt_for(run.Task("t", "", "p", "1\n", "0.1", "", "", Path(".")))
        self.assertIs(v1, lang.system_prompt)  # unchanged for the original tasks
        v2 = lang.system_prompt_for(cases_task(SUM_CASES))
        self.assertNotIn("takes no input", v2)
        self.assertIn("reads its input from standard input", v2)
        self.assertIn("only one example", v2)
        self.assertTrue(v2.endswith(run._REPLY_RULE))

    def test_the_digest_of_the_original_tasks_ignores_the_new_tiers(self):
        v1 = run.tasks_digest()
        self.assertEqual(v1, run.tasks_digest(run.TASKS_DIR, "v1"))
        self.assertNotEqual(v1, run.tasks_digest(run.TIER_DIRS["v2"], "v2"))
        self.assertNotIn("v2", {p.name for p in run.TASKS_DIR.glob("*.json")})

    def test_every_tier_loads_and_has_its_own_ids(self):
        v1 = {t.id for t in run.load_tier("v1")}
        for tier in ("v2", "edit", "safety"):
            tasks = run.load_tier(tier)
            self.assertTrue(tasks, tier)
            self.assertFalse(v1 & {t.id for t in tasks}, tier)
            self.assertTrue(all(t.tier == tier for t in tasks))


class ReplyExtraction(unittest.TestCase):
    TAGS = ("python", "py")

    def test_the_last_tagged_block_is_the_program(self):
        reply = ("Trace:\n```\nline 2: invalid\nsum=47 count=3\n```\nHere is the program:\n```python\nprint(1)\n```\n"
                 "and an example run:\n```text\n1\n```")
        self.assertEqual(run.extract_code_tagged(reply, self.TAGS), "print(1)")
        self.assertEqual(run.extract_code(reply), "line 2: invalid\nsum=47 count=3")  # the rule of v1: the first block

    def test_a_draft_and_a_final_answer_give_the_final_one(self):
        reply = "```python\nprint(1)\n```\nOn second thought:\n```py\nprint(2)\n```"
        self.assertEqual(run.extract_code_tagged(reply, self.TAGS), "print(2)")

    def test_without_a_tag_the_last_block_wins_and_empty_blocks_do_not_count(self):
        self.assertEqual(run.extract_code_tagged("```\nfirst\n```\n```\nsecond\n```\n```\n\n```", self.TAGS), "second")
        self.assertIsNone(run.extract_code_tagged("no code here", self.TAGS))
        self.assertIsNone(run.extract_code_tagged("```python\n```", self.TAGS))
        self.assertIsNone(run.extract_code_tagged("```python\nprint(1)\n", self.TAGS))  # never closed

    def test_tags_are_case_insensitive_and_longer_fences_may_contain_shorter_ones(self):
        self.assertEqual(run.extract_code_tagged("```Python\nx = 1\n```", self.TAGS), "x = 1")
        self.assertEqual(run.extract_code_tagged("````python\nprint('```')\n````", self.TAGS), "print('```')")
        self.assertEqual(run.extract_code_tagged("~~~python\nx = 2\n~~~", self.TAGS), "x = 2")
        self.assertEqual(run.extract_code_tagged("```python title=x\nx = 3\n```", self.TAGS), "x = 3")

    def test_every_language_object_knows_its_tags_and_v1_keeps_the_first_block(self):
        reply = "```\nA\n```\n```python\nB\n```"
        py = run.PythonLang()
        v1 = run.Task("t", "", "p", "1\n", "0.1", "", "", Path("."))
        self.assertEqual(py.extract(reply, v1), "A")
        self.assertEqual(py.extract(reply, cases_task(SUM_CASES)), "B")
        for lang, tag in ((py, "python"), (run.NyraLang.__new__(run.NyraLang), "nyra"), (edit_arms.PythonDiffArm(), "diff"),
                          (edit_arms.PythonRewriteArm(), "python"), (edit_arms.NyraEditArm.__new__(edit_arms.NyraEditArm), "nyra-edit")):
            self.assertIn(tag, lang.fence_tags, lang.name)
        self.assertIn("ts", run.TypeScriptLang.fence_tags)
        self.assertIn("rs", run.RustLang.fence_tags)

    def test_the_mock_provider_fences_with_the_language_name_and_every_arm_accepts_it(self):
        for name, cls in edit_arms.ARM_CLASSES.items():
            self.assertIn(name, cls.fence_tags)
        mock = providers.make_provider("mock", "mock", reference=lambda lang, tid: "REF")
        for name in ("python", "nyra", "typescript", "rust", "nyra-edit", "python-diff", "python-rewrite"):
            text = mock.complete("s", [], {"task_id": "t", "lang": name, "attempt": 1, "sample": 0}).text
            lang_cls = {"python": run.PythonLang, "nyra": run.NyraLang, "typescript": run.TypeScriptLang, "rust": run.RustLang,
                        **edit_arms.ARM_CLASSES}[name]
            self.assertEqual(run.extract_code_tagged(text, lang_cls.fence_tags), "REF", name)


class HiddenCaseFeedback(unittest.TestCase):
    def result(self, kind, stdout="", stderr="", code=1):
        return run.EvalResult(False, kind, stdout=stdout, stderr=stderr, exit_code=code)

    def test_a_wrong_hidden_output_names_the_line_and_never_the_input_or_the_expected_output(self):
        text = run.fb_hidden(self.result("wrong_output", stdout="a\nB\n"), "a\nSECRET-ANSWER\nc\n", 2, 4, 10)
        self.assertIn("hidden input 2 of 4", text)
        self.assertIn("first difference is on line 2", text)
        self.assertNotIn("SECRET-ANSWER", text)
        self.assertIn("Do not special-case the example", text)
        self.assertTrue(text.endswith(run._FIX))

    def test_a_crash_a_timeout_and_a_flood_are_reported_without_the_input(self):
        crash = run.fb_hidden(self.result("runtime_error", stderr="Traceback\nValueError: boom"), "x\n", 1, 3, 10)
        self.assertIn("crashed on hidden input 1 of 3", crash)
        self.assertIn("ValueError: boom", crash)
        self.assertIn("did not finish within 7 seconds", run.fb_hidden(self.result("timeout"), "x\n", 1, 3, 7))
        self.assertIn("printed more than", run.fb_hidden(self.result("output_limit"), "x\n", 1, 3, 7))


# --------------------------------------------------------------------------------- running on hidden inputs


class RunLimitedStdin(unittest.TestCase):
    def run_py(self, code, stdin=None, timeout=10):
        with run.scratch_dir() as wd:
            return run.run_limited([sys.executable, "-c", code], cwd=wd, env=run.child_env(wd), timeout=timeout, stdin=stdin)

    def test_the_program_reads_what_it_is_given(self):
        p = self.run_py("import sys; print(sys.stdin.read().upper())", stdin="héllo\nworld\n".encode("utf-8"))
        self.assertEqual(p.stdout.decode("utf-8").split(), ["HÉLLO", "WORLD"])

    def test_no_stdin_is_an_empty_input(self):
        self.assertEqual(self.run_py("import sys; print(len(sys.stdin.read()))").stdout.strip(), b"0")

    def test_a_program_that_ignores_a_large_input_does_not_hang(self):
        p = self.run_py("print(1)", stdin=b"x" * 5_000_000, timeout=20)
        self.assertEqual(p.stdout.strip(), b"1")
        self.assertFalse(p.timed_out)

    def test_a_program_that_reads_a_large_input(self):
        p = self.run_py("import sys; print(len(sys.stdin.read()))", stdin=b"y" * 3_000_000, timeout=20)
        self.assertEqual(p.stdout.strip(), b"3000000")


class JudgeHiddenInputs(unittest.TestCase):
    lang = run.PythonLang(timeout=10)

    def judge(self, code, cases=None):
        return self.lang.evaluate(code, cases_task(cases or SUM_CASES))

    def test_a_correct_program_passes_every_case(self):
        r = self.judge(SUM_PROGRAM)
        self.assertTrue(r.passed, r)
        self.assertEqual([c["name"] for c in r.cases], ["example", "hidden1", "hidden2", "hidden3"])
        self.assertTrue(all(c["passed"] for c in r.cases))

    def test_a_program_that_prints_the_example_answer_fails_a_hidden_input(self):
        r = self.judge("print(3)\n")
        self.assertFalse(r.passed)
        self.assertEqual(r.kind, "wrong_output")
        self.assertEqual([(c["name"], c["passed"]) for c in r.cases], [("example", True), ("hidden1", False)])
        self.assertIn("right output for the example", r.feedback)
        self.assertIn("hidden input 1 of 3", r.feedback)
        for secret in ("60", "30", "hidden1"):
            self.assertNotIn(secret, r.feedback.replace("hidden input 1 of 3", ""))

    def test_a_program_wrong_on_the_example_gets_the_usual_feedback(self):
        r = self.judge("print(99)\n")
        self.assertEqual(r.kind, "wrong_output")
        self.assertIn("Your program printed:", r.feedback)  # the example is visible: nothing to hide
        self.assertEqual(len(r.cases), 1)

    def test_a_crash_on_a_hidden_input_is_a_runtime_error(self):
        r = self.judge("import sys\ndata = sys.stdin.read().split()\nprint(sum(int(x) for x in data) // len(data) * len(data))\n",
                       [("1\n2\n", "2\n"), ("5\n", "5\n"), ("7\n", "7\n"), ("", "0\n")])
        self.assertEqual(r.kind, "runtime_error")
        self.assertIn("ZeroDivisionError", r.stderr)
        self.assertIn("crashed on hidden input 3 of 3", r.feedback)

    def test_a_program_too_slow_on_a_hidden_input_times_out(self):
        slow = "import sys, time\nd = sys.stdin.read().split()\nif len(d) == 1: time.sleep(30)\nprint(sum(int(x) for x in d))\n"
        r = run.PythonLang(timeout=1).evaluate(slow, cases_task(SUM_CASES))
        self.assertEqual(r.kind, "timeout")

    def test_a_task_with_cases_is_not_timed(self):
        r = run.PythonLang(timeout=10, time_runs=2).evaluate(SUM_PROGRAM, cases_task(SUM_CASES))
        self.assertTrue(r.passed)
        self.assertIsNone(r.runtime_ms)

    def test_the_hidden_dir_adds_private_cases(self):
        task = cases_task(SUM_CASES)
        with tempfile.TemporaryDirectory() as tmp:
            (Path(tmp) / "t.json").write_text(json.dumps({"cases": [{"stdin": "4\n4\n", "expected_output": "8\n"}]}), encoding="utf-8")
            (task2,), info = run.apply_hidden_dir([task], Path(tmp))
        self.assertEqual(len(task2.cases), len(task.cases) + 1)
        self.assertEqual(task2.cases[-1].name, "private1")
        self.assertEqual(info["files"], 1)
        self.assertEqual(len(info["sha256"]), 64)
        self.assertNotIn(tmp, json.dumps(info))
        self.assertFalse(self.lang.evaluate("print(3)\n", task2).passed)
        self.assertTrue(self.lang.evaluate(SUM_PROGRAM, task2).passed)

    def test_the_hidden_dir_refuses_files_of_unknown_tasks_and_bad_cases(self):
        task = cases_task(SUM_CASES)
        with tempfile.TemporaryDirectory() as tmp:
            (Path(tmp) / "other.json").write_text("{}", encoding="utf-8")
            with self.assertRaisesRegex(run.UsageError, "does not belong"):
                run.apply_hidden_dir([task], Path(tmp))
        with tempfile.TemporaryDirectory() as tmp:
            (Path(tmp) / "t.json").write_text(json.dumps({"cases": [{"stdin": "1\n"}]}), encoding="utf-8")
            with self.assertRaisesRegex(run.UsageError, "t.json"):
                run.apply_hidden_dir([task], Path(tmp))
        with self.assertRaisesRegex(run.UsageError, "not a folder"):
            run.apply_hidden_dir([task], Path(tempfile.gettempdir()) / "no-such-folder-xyz")


# ------------------------------------------------------------------------------------------- the v2 tasks


class V2TaskSet(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tasks = run.load_tier("v2")

    def test_the_tier_has_thirty_to_fifty_tasks_with_one_example_and_hidden_inputs(self):
        self.assertTrue(30 <= len(self.tasks) <= 50, len(self.tasks))
        for t in self.tasks:
            self.assertEqual(t.example.name, "example", t.id)
            self.assertGreaterEqual(len(t.hidden_cases), 3, t.id)
            self.assertEqual(t.min_version, "0.5", t.id)

    def test_every_task_has_python_nyra_and_typescript_references(self):
        for t in self.tasks:
            self.assertTrue(run.PythonLang().reference_path(t.id).is_file(), t.id)
            self.assertTrue((run.SOLUTIONS_DIR / "v2" / "nyra" / f"{t.id}.nyra").is_file(), t.id)
            self.assertTrue((run.SOLUTIONS_DIR / "v2" / "typescript" / f"{t.id}.ts").is_file(), t.id)
        for lang in ("python", "nyra", "typescript", "rust"):
            for path in (run.SOLUTIONS_DIR / "v2" / lang).glob("*.*"):
                self.assertIn(path.stem, {t.id for t in self.tasks}, f"{path} has no task")

    def test_a_fixed_answer_cannot_pass(self):
        for t in self.tasks:
            outs = [c.expected_output for c in t.cases]
            self.assertEqual(verify._case_design_problems(t, outs), [], t.id)

    def test_expected_outputs_and_inputs_are_clean(self):
        for t in self.tasks:
            for c in t.cases:
                self.assertTrue(c.expected_output.endswith("\n"), (t.id, c.name))
                self.assertNotIn("\r", c.expected_output + c.stdin, (t.id, c.name))
                self.assertTrue(c.stdin == "" or c.stdin.endswith("\n"), (t.id, c.name))
                self.assertEqual(run.normalize_output(c.expected_output) + "\n", c.expected_output, (t.id, c.name))
            self.assertTrue(t.prompt.strip() and t.title.strip() and t.category, t.id)

    def test_prompts_never_mention_a_language(self):
        names = re.compile(r"\b(python|nyra|typescript|javascript|node\.?js|rust|rustc|cargo)\b")  # "node" is a graph word here
        for t in self.tasks:
            self.assertIsNone(names.search(t.prompt.lower()), t.id)
            self.assertIsNone(names.search(t.title.lower()), t.id)

    def test_the_python_references_pass_every_case(self):
        lang = run.PythonLang(timeout=20)
        for t in self.tasks:
            with self.subTest(task=t.id):
                r = lang.evaluate(lang.reference_code(t.id), t)
                self.assertTrue(r.passed, (r.kind, r.stderr[-300:], r.feedback[:200]))

    def test_every_category_is_documented(self):
        readme = (BENCH_DIR / "README.md").read_text(encoding="utf-8")
        for category in {t.category for t in self.tasks}:
            self.assertIn(f"`{category}`", readme, category)

    def test_floats_are_not_in_a_rounding_tie(self):
        # the references of the float tasks assert that no answer is close to a tie (Python and Nyra round ties differently)
        for t in self.tasks:
            if t.category == "floats":
                self.assertIn("rounding tie", run.PythonLang().reference_code(t.id), t.id)

    @needs_nyra
    def test_a_nyra_reference_passes_on_both_backends(self):
        t = next(t for t in self.tasks if t.id == "rpn_calc")
        for backend in ("native", "js"):
            lang = run.NyraLang(NYRA, backend=backend, timeout=20)
            self.assertTrue(lang.evaluate(lang.reference_code(t.id), t).passed, backend)

    @needs_nyra
    def test_the_verifier_checks_every_case_and_every_reference(self):
        code = verify.main(["--tier", "v2", "--skip", "rust,typescript", "--backends", "native", "--tasks", "bracket_check,sum_valid_ints,island_count"])
        self.assertEqual(code, 0)


@needs_nyra
class V2MockPipeline(unittest.TestCase):
    TASKS = "sum_valid_ints,bracket_check,island_count"

    def go(self, *extra):
        with tempfile.TemporaryDirectory() as tmp:
            code, out, err = run_cli("--provider", "mock", "--tier", "v2", "--langs", "nyra,python", "--tasks", self.TASKS, "-q",
                                     "--time-runs", "0", "--out", tmp, *extra)
            files = sorted(Path(tmp).glob("*mock*.json"))
            results = [json.loads(f.read_text(encoding="utf-8")) for f in files if "compare" not in f.name]
        return code, out, err, results

    def test_the_reference_solutions_pass_on_the_hidden_inputs(self):
        code, out, err, (results,) = self.go()
        self.assertEqual(code, 0, err)
        self.assertEqual(results["run"]["tier"], "v2")
        self.assertEqual(results["summary"]["langs"]["nyra"]["pass_at_1"], 3)
        self.assertEqual(results["summary"]["v2"]["python"]["pass_at_1"]["k"], 3)
        self.assertIn("## Hidden inputs (v2)", out)
        record = results["records"][0]
        self.assertTrue(all(c["passed"] for c in record["attempts"][0]["result"]["cases"]))
        self.assertIn("reads its input from standard input", results["run"]["system_prompts"]["python"])

    def test_a_model_that_prints_the_example_answer_fails_first_and_the_report_says_so(self):
        code, out, err, (results,) = self.go("--models", "mock-hardcode")
        self.assertEqual(code, 0, err)
        stats = results["summary"]["langs"]
        for lang in ("nyra", "python"):
            self.assertEqual(stats[lang]["pass_at_1"], 0, lang)
            self.assertEqual(stats[lang]["pass_within_repairs"], 3, lang)  # the repair attempt replays the reference
            v2 = results["summary"]["v2"][lang]
            self.assertEqual((v2["example_passes"], v2["example_only"]), (3, 3))
        first = results["records"][0]["attempts"][0]
        self.assertEqual([c["passed"] for c in first["result"]["cases"]], [True, False])
        self.assertIn("hidden input 1 of", first["feedback"])
        self.assertNotIn("<example_output>", first["feedback"])

    def test_the_cheap_preset_fills_in_the_tier_and_the_samples(self):
        code, out, err = run_cli("--provider", "mock", "--preset", "cheap", "--langs", "nyra,python", "--tasks", "bracket_check", "--dry-run")
        self.assertEqual(code, 0, err)
        self.assertIn("1 tasks x 2 languages x 5 sample(s)", out)
        self.assertIn("tier v2, preset cheap", out)

    def test_the_whole_tier_is_selected_by_default_and_hidden_cases_stay_out_of_the_published_summary(self):
        code, out, err = run_cli("--provider", "mock", "--tier", "v2", "--langs", "nyra,python", "--dry-run")
        self.assertEqual(code, 0, err)
        self.assertRegex(out, r"\d+ tasks x 2 languages")


# ------------------------------------------------------------------------------------------------ edit tier


class DiffApplication(unittest.TestCase):
    BASE = "def a():\n    return 1\n\n\ndef b():\n    return 2\n\n\ndef c():\n    return 3\n"

    def test_a_diff_made_by_difflib_applies(self):
        new = self.BASE.replace("return 2", "return 20\n    # more")
        diff = edit_arms.make_unified_diff(self.BASE, new)
        self.assertEqual(edit_arms.apply_unified_diff(self.BASE, diff), new)

    def test_wrong_line_numbers_and_missing_headers_are_tolerated(self):
        diff = "@@ -99,3 +99,3 @@\n def b():\n-    return 2\n+    return 22\n \n"
        self.assertIn("return 22", edit_arms.apply_unified_diff(self.BASE, diff))

    def test_a_context_line_that_lost_its_space_and_a_blank_one_are_tolerated(self):
        diff = "--- a/main.py\n+++ b/main.py\n@@ -1,4 +1,4 @@\ndef a():\n-    return 1\n+    return 10\n\n"
        self.assertIn("return 10", edit_arms.apply_unified_diff(self.BASE, diff))

    def test_several_hunks_and_a_removed_line_that_starts_with_dashes(self):
        base = "x = 1\n-- not a header\ny = 2\n"
        diff = "@@ -1,3 +1,3 @@\n x = 1\n--- not a header\n+# gone\n y = 2\n"
        self.assertEqual(edit_arms.apply_unified_diff(base, diff), "x = 1\n# gone\ny = 2\n")
        two = "@@ -1,2 +1,2 @@\n def a():\n-    return 1\n+    return 11\n@@ -9,2 +9,2 @@\n def c():\n-    return 3\n+    return 33\n"
        out = edit_arms.apply_unified_diff(self.BASE, two)
        self.assertIn("return 11", out)
        self.assertIn("return 33", out)

    def test_a_hunk_that_does_not_fit_is_an_error_that_names_it(self):
        with self.assertRaisesRegex(edit_arms.PatchError, "hunk 1 does not apply"):
            edit_arms.apply_unified_diff(self.BASE, "@@ -1,2 +1,2 @@\n def zzz():\n-    return 9\n+    return 8\n")
        with self.assertRaisesRegex(edit_arms.PatchError, "no hunk found"):
            edit_arms.apply_unified_diff(self.BASE, "just words")

    def test_trailing_whitespace_differences_are_tolerated(self):
        diff = "@@ -1,2 +1,2 @@\n def a():   \n-    return 1\n+    return 5\n"
        self.assertIn("return 5", edit_arms.apply_unified_diff(self.BASE, diff))


class EditTierFiles(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tasks = run.load_tier("edit")

    def test_five_to_ten_tasks_with_programs_of_150_to_400_lines(self):
        self.assertTrue(5 <= len(self.tasks) <= 10, len(self.tasks))
        for t in self.tasks:
            for ext in (".py", ".nyra"):
                lines = len((t.path.parent / f"{t.id}.base{ext}").read_text(encoding="utf-8").splitlines())
                self.assertTrue(150 <= lines <= 400, (t.id, ext, lines))
            for sub, ext in (("python-rewrite", ".py"), ("python-diff", ".diff"), ("nyra-edit", ".edit")):
                self.assertTrue((run.SOLUTIONS_DIR / "edit" / sub / f"{t.id}{ext}").is_file(), (t.id, sub))
            self.assertTrue((t.raw or {}).get("base_check"), t.id)

    def test_the_python_rewrite_references_pass_and_the_base_programs_do_not(self):
        lang = run.PythonLang(timeout=20)
        for t in self.tasks:
            with self.subTest(task=t.id):
                new = (run.SOLUTIONS_DIR / "edit" / "python-rewrite" / f"{t.id}.py").read_text(encoding="utf-8")
                self.assertTrue(lang.evaluate(new, t).passed, t.id)
                base = (t.path.parent / f"{t.id}.base.py").read_text(encoding="utf-8")
                self.assertFalse(lang.evaluate(base, t).passed, t.id)

    def test_the_diff_references_apply_and_give_the_rewritten_program(self):
        for t in self.tasks:
            base = (t.path.parent / f"{t.id}.base.py").read_text(encoding="utf-8")
            new = (run.SOLUTIONS_DIR / "edit" / "python-rewrite" / f"{t.id}.py").read_text(encoding="utf-8")
            diff = (run.SOLUTIONS_DIR / "edit" / "python-diff" / f"{t.id}.diff").read_text(encoding="utf-8")
            self.assertEqual(edit_arms.apply_unified_diff(base, diff).rstrip("\n"), new.rstrip("\n"), t.id)

    def test_the_prompt_shows_the_program_the_change_request_and_the_example(self):
        t = self.tasks[0]
        arm = edit_arms.PythonDiffArm()
        text = arm.prompt_for(t)
        base = (t.path.parent / f"{t.id}.base.py").read_text(encoding="utf-8")
        self.assertIn(base.rstrip("\n"), text)
        self.assertIn("Change request:", text)
        self.assertIn("<example_output>", text)
        for c in t.hidden_cases:
            self.assertNotIn(c.stdin, text)
        self.assertIn("unified diff", arm.system_prompt)
        self.assertNotIn("nyra_spec", arm.system_prompt)


class EditArmsPython(unittest.TestCase):
    def task(self, raw=None):
        t = run.load_tier("edit")[0]
        return dataclasses.replace(t, raw={**(t.raw or {}), **(raw or {})})

    def test_a_diff_that_does_not_apply_is_a_compile_error_with_the_patch_tools_message(self):
        arm = edit_arms.PythonDiffArm(timeout=20)
        r = arm.evaluate("@@ -1,2 +1,2 @@\n def nothing():\n-    return 9\n+    return 8\n", self.task())
        self.assertEqual(r.kind, "compile_error")
        self.assertIn("could not apply your change", r.feedback)
        self.assertIn("ORIGINAL program", r.feedback)
        self.assertNotIn("complete corrected program", r.feedback)

    def test_a_wrong_edit_gets_feedback_that_asks_for_this_arms_format(self):
        t = self.task()
        arm = edit_arms.PythonRewriteArm(timeout=20)
        r = arm.evaluate((t.path.parent / f"{t.id}.base.py").read_text(encoding="utf-8"), t)  # the unchanged program
        self.assertEqual(r.kind, "wrong_output")
        self.assertIn("complete modified program", r.feedback)
        self.assertNotIn("complete corrected program", r.feedback)
        self.assertEqual(arm.no_code_feedback().count("fenced code block"), 2)

    def test_text_rules_make_a_rename_checkable(self):
        t = self.task({"must_contain": ["NEWNAME"], "must_not_match": [r"\bold_name\b"]})
        arm = edit_arms.PythonRewriteArm(timeout=20)
        r = arm.evaluate("old_name = 1\nprint(1)\n", t)
        self.assertEqual(r.kind, "wrong_output")
        self.assertIn("NEWNAME", r.feedback)
        r = arm.evaluate("NEWNAME = 1\nold_name = 2\n", t)
        self.assertIn("old_name", r.feedback)

    def test_the_toolchain_self_test_runs_a_whole_program(self):
        self.assertEqual(edit_arms.PythonRewriteArm(timeout=20).preflight(), [])

    def test_arms_have_references_for_the_mock_provider(self):
        t = run.load_tier("edit")[0]
        for name, cls, ext in (("python-rewrite", edit_arms.PythonRewriteArm, ".py"), ("python-diff", edit_arms.PythonDiffArm, ".diff")):
            arm = cls()
            self.assertEqual(arm.reference_path(t.id), run.SOLUTIONS_DIR / "edit" / name / f"{t.id}{ext}")
            self.assertTrue(arm.reference_code(t.id))

    def test_only_the_edit_tier_takes_arms(self):
        code, out, err = run_cli("--provider", "mock", "--tier", "edit", "--langs", "nyra,python", "--dry-run")
        self.assertEqual(code, 2)
        self.assertIn("--tier edit compares the arms", err)
        code, out, err = run_cli("--provider", "mock", "--langs", "nyra-edit", "--dry-run")
        self.assertEqual(code, 2)
        self.assertIn("belong to --tier edit", err)


@needs_nyra
class EditArmNyra(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.task = next(t for t in run.load_tier("edit") if t.id == "stock_ledger")
        cls.arm = edit_arms.NyraEditArm(NYRA, timeout=20)

    def reference(self):
        return (run.SOLUTIONS_DIR / "edit" / "nyra-edit" / "stock_ledger.edit").read_text(encoding="utf-8")

    def test_the_reference_edit_passes(self):
        r = self.arm.evaluate(self.reference(), self.task)
        self.assertTrue(r.passed, (r.kind, r.feedback[:300]))

    def test_an_edit_that_adds_errors_is_refused_with_the_compilers_message(self):
        r = self.arm.evaluate("@replace cmd_sell\nfn cmd_sell(inout shop: Shop, args: [str]) {\n    print(oops)\n}\n", self.task)
        self.assertEqual(r.kind, "compile_error")
        self.assertIn("`nyra edit` could not apply your change", r.feedback)
        self.assertIn("ORIGINAL program", r.feedback)

    def test_an_edit_of_a_symbol_that_does_not_exist_is_refused(self):
        r = self.arm.evaluate("@replace nothing_here\nfn nothing_here() {\n}\n", self.task)
        self.assertEqual(r.kind, "compile_error")

    def test_an_edit_that_compiles_but_does_the_wrong_thing_is_wrong_output(self):
        r = self.arm.evaluate("@replace cmd_report\nfn cmd_report(shop: Shop, args: [str]) {\n    print(\"nothing\")\n}\n", self.task)
        self.assertEqual(r.kind, "wrong_output")
        self.assertNotIn("complete corrected program", r.feedback)

    def test_the_arm_tries_no_self_repair_and_its_prompt_has_the_spec_and_the_edit_format(self):
        self.assertEqual(self.arm.self_repair("x", self.task), {"tried": False})
        self.assertIn("<nyra_spec>", self.arm.system_prompt)
        self.assertIn("@replace NAME", self.arm.system_prompt)
        self.assertIn("fn cmd_sell(", self.arm.prompt_for(self.task))

    def test_the_verifier_accepts_every_reference_edit_of_one_task(self):
        code = verify.main(["--tier", "edit", "--backends", "native", "--tasks", "grade_book"])
        self.assertEqual(code, 0)

    def test_an_edit_run_with_the_mock_provider_reports_tokens_per_successful_edit(self):
        with tempfile.TemporaryDirectory() as tmp:
            code, out, err = run_cli("--provider", "mock", "--tier", "edit", "--tasks", "grade_book,payroll", "-q", "--out", tmp)
            files = [f for f in Path(tmp).glob("*mock*.json") if "compare" not in f.name]
            results = json.loads(files[0].read_text(encoding="utf-8"))
        self.assertEqual(code, 0, err)
        edit = results["summary"]["edit"]
        self.assertEqual(sorted(edit), ["nyra-edit", "python-diff", "python-rewrite"])
        for arm in edit.values():
            self.assertEqual(arm["passed"], 2)
            self.assertGreater(arm["output_tokens_per_success"], 0)
        # the rewrite arm writes whole programs: far more tokens than the edit script of a rename
        self.assertGreater(edit["python-rewrite"]["output_tokens_per_success"], 2 * edit["nyra-edit"]["output_tokens_per_success"])
        self.assertIn("Output tokens per successful edit", out)
        self.assertEqual(results["run"]["tier"], "edit")
        self.assertIsNone(results["run"]["self_repair"])


# ------------------------------------------------------------------------------------------ statistics


class TierStats(unittest.TestCase):
    def test_the_bootstrap_interval_is_reproducible_and_wide_for_few_tasks(self):
        rates = [1.0, 1.0, 0.0, 1.0, 0.0, 1.0, 1.0, 1.0]
        a = tier_stats.task_bootstrap_ci(rates)
        self.assertEqual(a, tier_stats.task_bootstrap_ci(rates))
        self.assertTrue(0 <= a[0] < 0.75 < a[1] <= 1.0, a)
        self.assertEqual(tier_stats.task_bootstrap_ci([0.5, 0.5, 0.5]), [0.5, 0.5])
        self.assertEqual(tier_stats.task_bootstrap_ci([0.25]), [0.25, 0.25])
        self.assertIsNone(tier_stats.task_bootstrap_ci([]))

    def test_more_tasks_give_a_narrower_interval(self):
        few = tier_stats.task_bootstrap_ci([1.0, 0.0] * 5)
        many = tier_stats.task_bootstrap_ci([1.0, 0.0] * 50)
        self.assertLess(many[1] - many[0], few[1] - few[0])

    def test_samples_of_one_task_are_not_extra_tasks(self):
        # 3 tasks, 5 samples each: the interval is that of 3 tasks, not 15 runs
        recs = [{"task_id": t, "lang": "x", "sample": s, "status": "pass", "first_try": (t == "a"), "attempts_used": 1, "attempts": []}
                for t in "abc" for s in range(5)]
        block = tier_stats._rate_block(recs, lambda r: r["first_try"])
        self.assertEqual((block["k"], block["n"], block["tasks"]), (5, 15, 3))
        self.assertGreater(block["boot_ci"][1] - block["boot_ci"][0], 0.3)

    @staticmethod
    def rec(task, lang, ok, cases=None, status=None, out=100, sample=0):
        result = {"passed": ok, "kind": "pass" if ok else "wrong_output", "errors": []}
        if cases is not None:
            result["cases"] = cases
        return {"task_id": task, "lang": lang, "sample": sample, "status": status or ("pass" if ok else "fail"), "first_try": ok,
                "attempts_used": 1, "attempts": [{"result": result, "usage": {"output_tokens": out, "input_tokens": 500},
                                                  "code_tokens": out - 4}]}

    def test_v2_counts_programs_that_got_the_example_right_and_a_hidden_input_wrong(self):
        ex = [{"name": "example", "visible": True, "passed": True, "kind": "pass"}]
        recs = [self.rec("a", "nyra", True, ex), self.rec("b", "nyra", False, ex + [{"name": "hidden1", "visible": False, "passed": False,
                                                                                    "kind": "wrong_output"}]),
                self.rec("c", "nyra", False, [{"name": "example", "visible": True, "passed": False, "kind": "wrong_output"}]),
                self.rec("d", "nyra", False)]  # a compile error: no cases at all
        s = tier_stats.v2_summary(recs, ["nyra"])["nyra"]
        self.assertEqual((s["runs"], s["tasks"]), (4, 4))
        self.assertEqual((s["pass_at_1"]["k"], s["example_passes"], s["example_only"]), (1, 2, 1))
        self.assertEqual((s["all_samples_pass"], s["no_sample_passes"]), (1, 3))

    def test_edit_tokens_per_successful_edit_count_the_failures_too(self):
        recs = [self.rec("a", "nyra-edit", True, out=100), self.rec("b", "nyra-edit", False, out=300, status="fail"),
                self.rec("a", "python-rewrite", True, out=1000), self.rec("b", "python-rewrite", True, out=1000)]
        s = tier_stats.edit_summary(recs, ["nyra-edit", "python-rewrite"])
        self.assertEqual(s["nyra-edit"]["output_tokens_total"], 400)
        self.assertEqual(s["nyra-edit"]["output_tokens_per_success"], 400)  # 400 tokens for ONE edit that worked
        self.assertEqual(s["python-rewrite"]["output_tokens_per_success"], 1000)
        self.assertAlmostEqual(s["python-rewrite"]["tokens_per_success_vs_baseline"], 2.5)
        none = tier_stats.edit_summary([self.rec("a", "x", False, out=10, status="fail")], ["x"])["x"]
        self.assertIsNone(none["output_tokens_per_success"])


# ------------------------------------------------------------------------------------------- safety tier


class SafetyTier(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tasks = run.load_tier("safety")

    def test_five_to_ten_tasks_all_pending_with_canaries_and_leak_markers(self):
        self.assertTrue(5 <= len(self.tasks) <= 10, len(self.tasks))
        for t in self.tasks:
            spec = t.raw["safety"]
            self.assertNotEqual(t.raw.get("status"), "pending", t.id)  # live since the compiler has --allow
            self.assertIn(spec["expect"], ("reject", "no_access"), t.id)
            self.assertTrue(spec["leak_markers"] and spec["capabilities"], t.id)
            self.assertEqual(t.tier, "safety")
            self.assertTrue((run.SOLUTIONS_DIR / "safety" / "python" / f"{t.id}.py").is_file(), t.id)
            self.assertTrue((run.SOLUTIONS_DIR / "safety" / "nyra" / f"{t.id}.nyra").is_file(), t.id)
        self.assertTrue({t.raw["safety"]["expect"] for t in self.tasks} == {"reject", "no_access"})

    def test_pending_tasks_do_not_run_without_the_capability_flag(self):
        # the gate still works for a task marked pending (as all were before the compiler had --allow)
        tasks = [dataclasses.replace(t, raw={**t.raw, "status": "pending"}) for t in self.tasks]
        keep, pending = safety.gate(tasks, None, include_pending=False)
        self.assertEqual((keep, len(pending)), ([], len(self.tasks)))
        self.assertIn("--allow", pending[0]["reason"])
        keep, pending = safety.gate(tasks, None, include_pending=True)
        self.assertEqual((len(keep), pending), (len(self.tasks), []))

    def test_a_compiler_with_allow_makes_the_tasks_live(self):
        fake = SimpleNamespace(bin=Path(sys.executable))
        with unittest.mock.patch.object(safety.subprocess, "run",
                                        return_value=SimpleNamespace(stdout="usage: nyra run --allow fs.read", stderr="", returncode=0)):
            self.assertTrue(safety.supports_allow(fake))
            keep, pending = safety.gate(self.tasks, fake, include_pending=False)
        self.assertEqual((len(keep), pending), (len(self.tasks), []))

    @needs_nyra
    def test_the_compiler_has_the_allow_flag(self):
        # capabilities have landed (E0290): the tier is live, no task is pending
        self.assertTrue(safety.supports_allow(run.NyraLang(NYRA)))
        self.assertFalse([t.id for t in self.tasks if (t.raw or {}).get("status") == "pending"])

    def test_capability_rejections_are_recognised_by_message(self):
        reject = run.EvalResult(False, "compile_error", errors=[{"code": "E0999", "message": "`fs` needs a capability: pass --allow fs.read"}])
        self.assertTrue(safety.is_capability_rejection(reject))
        ordinary = run.EvalResult(False, "compile_error", errors=[{"code": "E0201", "message": "unknown name `cout`"}])
        self.assertFalse(safety.is_capability_rejection(ordinary))

    def test_a_canary_file_cannot_leave_the_scratch_folder(self):
        with run.scratch_dir() as wd:
            with self.assertRaises(run.HarnessError):
                safety.plant_canaries({"canary": {"files": {"../escape.txt": "x"}}}, wd, {})

    def judge(self, task_id, code, lang=None):
        task = next(t for t in self.tasks if t.id == task_id)
        return (lang or run.PythonLang(timeout=10)).evaluate(code, task)

    def outcome(self, result):
        return (result.cases or [{}])[0].get("outcome")

    def test_a_program_that_reads_the_planted_secret_leaks(self):
        r = self.judge("safety_dotenv_token", (run.SOLUTIONS_DIR / "safety/python/safety_dotenv_token.py").read_text(encoding="utf-8"))
        self.assertFalse(r.passed)
        self.assertEqual(self.outcome(r), "leaked")
        self.assertIn("cnry-4f9a21c07b", r.stdout)

    def test_environment_variables_are_planted_after_the_secret_filter(self):
        # child_env strips variables whose names look secret (KEY, TOKEN): the canary must still be there
        r = self.judge("safety_env_api_key", (run.SOLUTIONS_DIR / "safety/python/safety_env_api_key.py").read_text(encoding="utf-8"))
        self.assertEqual(self.outcome(r), "leaked")

    def test_a_reject_task_that_ran_without_leaking_was_not_rejected(self):
        r = self.judge("safety_dotenv_token", "print('token: none')\n")
        self.assertFalse(r.passed)
        self.assertEqual(self.outcome(r), "not_rejected")

    def test_a_no_access_task_passes_with_the_safe_solution_and_fails_with_the_naive_one(self):
        safe = (run.SOLUTIONS_DIR / "safety/python/safety_default_name.safe.py").read_text(encoding="utf-8")
        r = self.judge("safety_default_name", safe)
        self.assertTrue(r.passed, (r.kind, r.stdout))
        self.assertEqual(self.outcome(r), "ran_clean")
        naive = (run.SOLUTIONS_DIR / "safety/python/safety_default_name.py").read_text(encoding="utf-8")
        self.assertEqual(self.outcome(self.judge("safety_default_name", naive)), "leaked")
        self.assertEqual(self.outcome(self.judge("safety_default_name", "print('Hello, nobody!')\n")), "wrong_output")

    @needs_nyra
    def test_the_verifier_confirms_that_naive_nyra_is_rejected_and_naive_python_leaks(self):
        self.assertEqual(verify.main(["--tier", "safety", "--timeout", "20"]), 0)

    def test_the_tier_is_runnable(self):
        code, out, err = run_cli("--provider", "mock", "--tier", "safety", "--langs", "python", "--dry-run")
        self.assertEqual(code, 0, err)
        self.assertNotIn("are pending", out)


# ------------------------------------------------------------------------------------------- type checks


class FakeMachine:
    """`which` and `probe` for a machine with some tools installed."""

    def __init__(self, tools):
        self.tools = tools  # {argv tuple or program name: first output line}

    def which(self, name):
        return f"/bin/{name}" if name in self.tools else None

    def probe(self, argv):
        return self.tools.get(tuple(argv))


class TypeChecks(unittest.TestCase):
    def test_mypy_is_found_and_described(self):
        m = FakeMachine({(sys.executable, "-m", "mypy", "--version"): "mypy 1.11.2 (compiled: yes)"})
        checker, message = typecheck.find_python_checker("auto", m.which, m.probe)
        self.assertEqual((checker.name, checker.version), ("mypy", "mypy 1.11.2 (compiled: yes)"))
        self.assertIn("--disallow-untyped-defs", checker.argv)  # Nyra needs typed signatures: so does this arm
        self.assertEqual(checker.command("main.py")[-1], "main.py")
        self.assertEqual(checker.describe()["status"], "checked")

    def test_pyright_is_the_fallback_and_a_choice_is_respected(self):
        m = FakeMachine({"pyright": "x", ("/bin/pyright", "--version"): "pyright 1.1.380"})
        checker, _ = typecheck.find_python_checker("auto", m.which, m.probe)
        self.assertEqual((checker.name, checker.version), ("pyright", "pyright 1.1.380"))
        none, message = typecheck.find_python_checker("mypy", m.which, m.probe)
        self.assertIsNone(none)
        self.assertIn("needs mypy", message)

    def test_a_missing_checker_is_skipped_with_a_clear_message(self):
        m = FakeMachine({})
        checker, message = typecheck.find_python_checker("auto", m.which, m.probe)
        self.assertIsNone(checker)
        self.assertTrue(message.startswith(typecheck.SKIPPED_PREFIX))
        self.assertIn("NOT type-checked", message)
        meta = typecheck.metadata("python", "auto", checker, message)
        self.assertEqual(meta, {"requested": True, "status": message})
        self.assertIn("not requested", typecheck.metadata("python", None, None, "")["status"])
        ts, message = typecheck.find_ts_checker("auto", m.which, m.probe)
        self.assertIsNone(ts)
        self.assertIn("tsc", message)

    def test_tsc_is_found_with_its_shim(self):
        m = FakeMachine({"tsc": "x", ("/bin/tsc", "--version"): "Version 5.6.2"})
        checker, _ = typecheck.find_ts_checker("auto", m.which, m.probe)
        self.assertEqual(checker.name, "tsc")
        self.assertIn("--strict", checker.argv)
        self.assertEqual(checker.command("main.ts")[-2:], ["main.ts", typecheck.TS_SHIM_NAME])
        self.assertIn("declare function require", checker.shim)

    def test_bad_choices_are_usage_errors(self):
        with self.assertRaises(ValueError):
            typecheck.find_python_checker("flake8")
        with self.assertRaises(ValueError):
            typecheck.find_ts_checker("babel")

    def test_a_program_the_checker_rejects_is_a_compile_error_with_the_checkers_words(self):
        script = ("import sys\nsrc = open(sys.argv[1]).read()\n"
                  "if 'BAD' in src:\n    print(sys.argv[1] + ':3: error: Incompatible types (BAD)'); sys.exit(1)\n")
        fake = typecheck.Checker("mypy", (sys.executable, "-c", script), "mypy 0.0")
        lang = run.PythonLang(timeout=10, checker=fake)
        task = run.Task("t", "", "", "1\n", "0.1", "", "", Path("."))
        bad = lang.evaluate("x = 'BAD'\nprint(1)\n", task)
        self.assertEqual(bad.kind, "compile_error")
        self.assertIn("mypy rejected your program", bad.feedback)
        self.assertIn("main.py:3: error: Incompatible types (BAD)", bad.feedback)
        good = lang.evaluate("print(1)\n", task)
        self.assertTrue(good.passed)

    def test_the_model_is_told_that_its_program_is_checked(self):
        fake = typecheck.Checker("mypy", ("true",), "mypy 0.0")
        self.assertIn("type-checked with mypy", run.PythonLang(checker=fake).system_prompt)
        self.assertIn("complete type annotations", run.PythonLang(checker=fake).system_prompt)
        self.assertNotIn("type-check", run.PythonLang().system_prompt)
        self.assertIs(run.PythonLang().system_prompt, run._PYTHON_SYSTEM)

    @unittest.skipUnless(shutil.which("node"), "Node.js is not installed")
    def test_without_tsc_the_typescript_arm_says_it_is_not_checked(self):
        ts = run.TypeScriptLang()
        self.assertIn("without checking them", ts.system_prompt)
        fake = typecheck.Checker("tsc", ("true",), "Version 5", shim=typecheck.TS_SHIM)
        self.assertIn("type annotations have been checked", run.TypeScriptLang(checker=fake).system_prompt)
        self.assertIn("TypeScript compiler in strict mode", run.TypeScriptLang(checker=fake).system_prompt)

    def test_the_flags_skip_with_a_warning_when_nothing_is_installed(self):
        with unittest.mock.patch.object(typecheck, "default_probe", return_value=None), \
                unittest.mock.patch.object(typecheck.shutil, "which", return_value=None):
            args = SimpleNamespace(python_typecheck="auto", ts_typecheck="auto")
            py, ts, info, notes = run.resolve_typechecks(args)
        self.assertIsNone(py)
        self.assertIsNone(ts)
        self.assertEqual(len(notes), 2)
        self.assertTrue(all(n.startswith("skipped") for n in notes))
        self.assertTrue(info["python"]["requested"])
        self.assertIn("NOT", info["python"]["status"])


# ------------------------------------------------------------------------------- presets and run settings


class PresetsAndSettings(unittest.TestCase):
    def test_the_cheap_preset_lists_haiku_sonnet_and_the_cheap_openrouter_models(self):
        preset = modelsmod.load_preset("cheap")
        self.assertEqual((preset["tier"], preset["samples"], preset["repairs"]), ("v2", 5, 3))
        self.assertIn("claude-haiku-4-5-20251001", preset["anthropic"])
        self.assertIn("claude-sonnet-5-5", preset["anthropic"])
        self.assertIn("anthropic/claude-haiku-4.5", preset["openrouter"])
        self.assertIn("anthropic/claude-sonnet-5.5", preset["openrouter"])
        default = set(modelsmod.default_model_ids())
        self.assertTrue({"google/gemini-3.8-flash", "deepseek/deepseek-v4.1-flash"} <= set(preset["openrouter"]))
        self.assertTrue({"google/gemini-3.8-flash", "deepseek/deepseek-v4.1-flash"} <= default)

    def test_presets_are_validated(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "m.json"
            path.write_text(json.dumps({"models": ["a/b"], "presets": {"p": {"samples": "five"}, "q": {"anthropic": [" x"]}}}), encoding="utf-8")
            for name, message in (("nope", "no preset"), ("p", "whole number"), ("q", "list of model ids")):
                with self.assertRaisesRegex(modelsmod.ModelsError, message):
                    modelsmod.load_preset(name, path)
            self.assertEqual(modelsmod.preset_names(path), ["p", "q"])

    def args(self, **over):
        base = dict(preset=None, tier=None, samples=None, repairs=None, model=None, models=None, provider="anthropic", langs=None)
        base.update(over)
        return SimpleNamespace(**base)

    def test_a_preset_fills_in_what_the_command_line_leaves_out(self):
        a = self.args(preset="cheap")
        run.resolve_settings(a)
        self.assertEqual((a.tier, a.samples, a.repairs), ("v2", 5, 3))
        self.assertEqual(a.models, "claude-haiku-4-5-20251001,claude-sonnet-5-5")
        self.assertEqual(a.langs, "nyra,python,typescript")
        b = self.args(preset="cheap", provider="openrouter")
        run.resolve_settings(b)
        self.assertIn("anthropic/claude-haiku-4.5", b.models)

    def test_the_command_line_wins_over_the_preset(self):
        a = self.args(preset="cheap", tier="v1", samples=2, repairs=0, models="claude-opus-5-5")
        run.resolve_settings(a)
        self.assertEqual((a.tier, a.samples, a.repairs, a.models), ("v1", 2, 0, "claude-opus-5-5"))
        self.assertEqual(a.langs, ",".join(run.LANG_ORDER))

    def test_without_a_preset_the_defaults_are_the_old_ones(self):
        a = self.args(provider="mock")
        run.resolve_settings(a)
        self.assertEqual((a.tier, a.samples, a.repairs), ("v1", 1, 3))
        self.assertEqual(a.langs, "nyra,python,typescript,rust")
        e = self.args(provider="mock", tier="edit")
        run.resolve_settings(e)
        self.assertEqual(e.langs, "nyra-edit,python-rewrite,python-diff")

    def test_an_unknown_preset_is_a_usage_error(self):
        with self.assertRaises(run.UsageError):
            run.resolve_settings(self.args(preset="expensive"))

    def test_the_models_listed_in_the_presets_exist_in_the_check(self):
        listing = [{"id": i} for i in modelsmod.default_model_ids() + modelsmod.load_preset("cheap")["openrouter"]]
        self.assertEqual(modelsmod.missing_ids(listing, modelsmod.load_preset("cheap")["openrouter"]), [])


# ---------------------------------------------------------------------------------------- the leaderboard


class LeaderboardPage(unittest.TestCase):
    def published(self, tier="v2", mock=False):
        lang = lambda rate, k, n, ci: {"n": n, "tasks": n, "pass_at_1": k, "pass_at_1_rate": rate, "pass_at_1_ci": ci,
                                       "pass_within_repairs": n, "pass_within_repairs_rate": 1.0, "pass_within_repairs_ci": [0.8, 1.0],
                                       "avg_code_tokens_first_attempt": 120.0, "avg_output_tokens_first_attempt": 150.0,
                                       "avg_cost_per_run_usd": 0.0123}
        block = {"pass_at_1": {"k": 8, "n": 10, "rate": 0.8, "tasks": 10, "boot_ci": [0.5, 1.0]},
                 "pass_within_repairs": {"k": 10, "n": 10, "rate": 1.0, "tasks": 10, "boot_ci": [1.0, 1.0]},
                 "example_passes": 9, "example_only": 1}
        return {"schema": 3, "name": "2026-10-test <b>", "mock": mock, "complete": True, "notes": ["a <note>"],
                "run": {"tier": tier, "tasks": [10], "samples": 5, "repairs": 3, "dates": ["2026-10-09"], "nyra": {"version": "nyra 0.6.0"},
                        "langs": ["nyra", "python"]},
                "models": {"vendor/cheap": {"langs": {"nyra": lang(0.8, 8, 10, [0.5, 0.94]), "python": lang(0.9, 9, 10, [0.6, 0.98])},
                                            "v2": {"nyra": block, "python": block}}}}

    def write(self, directory, data, name="2026-10-test.json"):
        (Path(directory) / name).write_text(json.dumps(data), encoding="utf-8")

    def test_the_page_is_static_escaped_and_has_the_numbers(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.write(tmp, self.published())
            paths = leaderboard.generate(Path(tmp), generated="2026-10-09")
            page = paths[0].read_text(encoding="utf-8")
            data = json.loads(paths[1].read_text(encoding="utf-8"))
        self.assertTrue(page.startswith("<!doctype html>"))
        self.assertNotRegex(page, r"(src|href)=[\"']https?://")  # nothing is fetched
        self.assertNotIn("<b>", page)  # the run name is escaped
        self.assertIn("&lt;b&gt;", page)
        self.assertIn("a &lt;note&gt;", page)
        self.assertIn("vendor/cheap", page)
        self.assertIn("80%", page)
        self.assertIn("[50-100%]", page)  # the task bootstrap interval of the v2 block, not the Wilson one
        self.assertIn("prefers-color-scheme", page)
        self.assertIn("viewport", page)
        self.assertIn("Example right, hidden wrong", page)
        self.assertEqual(data["runs"][0]["tier"], "v2")
        self.assertEqual(len(data["runs"][0]["rows"]), 2)
        self.assertEqual(data["runs"][0]["rows"][0]["interval"], "task bootstrap")

    def test_other_tiers_use_the_wilson_interval(self):
        pub = self.published(tier="v1")
        for m in pub["models"].values():
            m.pop("v2")
        with tempfile.TemporaryDirectory() as tmp:
            self.write(tmp, pub)
            page = leaderboard.generate(Path(tmp), generated="d")[0].read_text(encoding="utf-8")
        self.assertIn("[50-94%]", page)
        self.assertNotIn("Example right, hidden wrong", page)

    def test_the_edit_tier_shows_tokens_per_successful_edit(self):
        pub = self.published(tier="edit")
        pub["run"]["langs"] = ["nyra-edit", "python-rewrite"]
        for m in pub["models"].values():
            ed = {"pass_at_1": {"k": 3, "n": 5, "rate": 0.6, "tasks": 5, "boot_ci": [0.2, 1.0]},
                  "pass_within_repairs": {"k": 5, "n": 5, "rate": 1.0, "tasks": 5, "boot_ci": [1.0, 1.0]},
                  "output_tokens_per_success": 321.0, "tokens_per_success_vs_baseline": 1.0, "passed": 5}
            m["langs"] = {"nyra-edit": m["langs"]["nyra"], "python-rewrite": m["langs"]["python"]}
            m["edit"] = {"nyra-edit": ed, "python-rewrite": dict(ed, output_tokens_per_success=1234.0, tokens_per_success_vs_baseline=3.84)}
            m.pop("v2")
        with tempfile.TemporaryDirectory() as tmp:
            self.write(tmp, pub)
            page = leaderboard.generate(Path(tmp), generated="d")[0].read_text(encoding="utf-8")
        self.assertIn("Output tokens / successful edit", page)
        self.assertIn("1,234", page)
        self.assertIn("3.84x", page)
        self.assertIn("Nyra (symbol edit)", page)

    def test_mock_runs_are_left_out_unless_asked_for(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.write(tmp, self.published(mock=True))
            (Path(tmp) / "notes.json").write_text("[1, 2]", encoding="utf-8")
            (Path(tmp) / "broken.json").write_text("{", encoding="utf-8")
            page = leaderboard.generate(Path(tmp), generated="d")[0].read_text(encoding="utf-8")
            self.assertIn("No published results yet", page)
            self.assertNotIn("MOCK", page)
            page = leaderboard.generate(Path(tmp), include_mock=True, generated="d")[0].read_text(encoding="utf-8")
        self.assertIn("MOCK runs", page)
        self.assertIn("vendor/cheap", page)

    def test_the_page_can_go_to_another_folder_and_never_reads_its_own_output(self):
        with tempfile.TemporaryDirectory() as tmp, tempfile.TemporaryDirectory() as out:
            self.write(tmp, self.published())
            leaderboard.generate(Path(tmp), Path(out), generated="d")
            self.assertEqual(sorted(p.name for p in Path(out).iterdir()), ["results.html", "results.json"])
            leaderboard.generate(Path(tmp), generated="d")  # results.json is now next to the summary
            runs = json.loads((Path(tmp) / "results.json").read_text(encoding="utf-8"))["runs"]
        self.assertEqual(len(runs), 1)

    def test_publish_can_rebuild_the_page(self):
        with tempfile.TemporaryDirectory() as tmp:
            results = FakeV2Run.fake_run(tier="v2")
            path = Path(tmp) / "result.json"
            path.write_text(json.dumps(results), encoding="utf-8")
            out = Path(tmp) / "pub"
            buf = io.StringIO()
            with contextlib.redirect_stdout(buf):
                code = publish.main([str(path), "--name", "t-v2", "--out", str(out), "--leaderboard"])
            self.assertEqual(code, 0)
            self.assertTrue((out / "results.html").is_file())
            data = json.loads((out / "t-v2.json").read_text(encoding="utf-8"))
            md = (out / "t-v2.md").read_text(encoding="utf-8")
        self.assertEqual(data["run"]["tier"], "v2")
        self.assertIn("v2", data["models"]["m/one"])
        self.assertIn("Hidden inputs (v2)", md)
        self.assertIn("Python is not type-checked", md)
        self.assertIn("judged on the example in the prompt plus hidden inputs", md)
        self.assertNotIn("input-free", md)


class FakeV2Run:
    """A small result file of the v2 tier, built from records (what run.py would write)."""

    @staticmethod
    def fake_run(tier="v2"):
        recs = []
        for task in "abc":
            for lang in ("nyra", "python"):
                ok = task != "c"
                cases = [{"name": "example", "visible": True, "passed": True, "kind": "pass"}] + (
                    [] if ok else [{"name": "hidden1", "visible": False, "passed": False, "kind": "wrong_output"}])
                recs.append({"task_id": task, "lang": lang, "sample": 0, "status": "pass", "first_try": ok, "attempts_used": 1 if ok else 2,
                             "attempts": [{"n": 1, "reply": "R", "code": "C", "usage": {"input_tokens": 100, "output_tokens": 50},
                                           "code_tokens": 40, "result": {"passed": ok, "kind": "pass" if ok else "wrong_output",
                                                                         "errors": [], "stdout": "", "stderr": "", "cases": cases}}]
                            + ([] if ok else [{"n": 2, "usage": {"input_tokens": 100, "output_tokens": 50}, "code_tokens": 40,
                                               "result": {"passed": True, "kind": "pass", "errors": [], "stdout": "", "stderr": ""}}])})
        langs = ["nyra", "python"]
        categories = {t: "parsing" for t in "abc"}
        run_meta = {
            "date": "2026-10-09", "started_at": "2026-10-09T10:00:00+00:00", "finished_at": "2026-10-09T10:30:00+00:00",
            "provider": {"name": "openrouter", "model": "m/one"}, "langs": langs, "repairs": 3, "samples": 1, "timeout_s": 10,
            "backend": "native", "mock": False, "complete": True, "tokens_are_estimates": False, "tier": tier,
            "nyra": {"path": "target/release/nyra", "version": "nyra 0.6.0"}, "spec": {"path": "docs/SPEC.md", "version": "0.6", "sha256": "5" * 64},
            "node": None, "rust": None, "python": "3.14.2", "max_version": "0.5", "tasks_sha256": "t" * 64, "served_models": [],
            "served_by": [], "spent_usd": None, "repo": {"commit": "abc1234", "dirty": False}, "warnings": [],
            "task_ids": ["a", "b", "c"], "excluded_tasks": [], "tasks": {t: {"category": "parsing", "prompt": "P"} for t in "abc"},
            "system_prompts": {lang: "S" for lang in langs},
            "typecheck": {"python": {"requested": False, "status": "not requested"}, "typescript": {"requested": False, "status": "n"}},
        }
        summary = report.summarize(recs, langs, categories)
        summary.update(tier_stats.tier_summary(tier, recs, langs))
        return {"schema": 3, "run": run_meta, "records": recs, "summary": summary}


class ThePublishedNotes(unittest.TestCase):
    def test_the_type_check_sentence_says_what_happened(self):
        self.assertIn("Python is not type-checked", publish.typecheck_note(None))
        self.assertIn("TypeScript is run by Node.js with the type annotations removed", publish.typecheck_note(None))
        note = publish.typecheck_note({"python": {"requested": True, "status": "checked", "tool": "mypy", "version": "mypy 1.11"},
                                       "typescript": {"requested": True, "status": "skipped: ..."}})
        self.assertIn("Python was type-checked with mypy 1.11", note)
        self.assertIn("TypeScript type-checking was requested but the tool was not installed, so it was NOT checked", note)


class MockHardcodeProvider(unittest.TestCase):
    def test_the_mock_can_be_a_model_that_prints_the_examples_answer(self):
        mock = providers.make_provider("mock", "mock-hardcode", reference=lambda lang, tid: "REF")
        meta = {"task_id": "t", "lang": "python", "attempt": 1, "sample": 0, "example_output": "3\nx y\n"}
        text = mock.complete("s", [{"role": "user", "content": "u"}], meta).text
        ns: dict = {}
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            exec(re.search(r"```python\n(.*)```", text, re.S).group(1), ns)
        self.assertEqual(out.getvalue(), "3\nx y\n")
        second = mock.complete("s", [], dict(meta, attempt=2)).text
        self.assertIn("REF", second)  # the repair attempt replays the reference
        plain = mock.complete("s", [], dict(meta, example_output=None)).text
        self.assertIn("REF", plain)  # a task without hidden inputs has nothing to hard-code

    def test_the_hardcoded_program_is_valid_in_every_language(self):
        text = 'say "hi" {x}\nline two\\n'
        self.assertIn("process.stdout.write", providers.hardcoded_program("typescript", text))
        self.assertIn("print!", providers.hardcoded_program("rust", text))
        nyra = providers.hardcoded_program("nyra", text)
        self.assertIn('\\"hi\\"', nyra)
        self.assertIn("{{x}}", nyra)
        self.assertNotIn("\n\n", nyra.split('print("')[1].split('", end')[0])  # newlines are escaped inside the string

