"""Tests for the benchmark harness itself (not for Nyra).

    python bench/test_bench.py             # or: python -m unittest discover -s bench

Standard library only. Tests that need the Nyra compiler, Node.js or a Rust toolchain are skipped when
that tool is missing; the SDK test is skipped when the `anthropic` package is not importable. No test
calls a real API: the OpenRouter provider is tested against a scripted transport and against a stub
server on this machine (127.0.0.1), never against openrouter.ai.
"""

from __future__ import annotations

import concurrent.futures as cf
import contextlib
import hashlib
import http.client
import http.server
import io
import json
import os
import re
import shutil
import socket
import sys
import tempfile
import threading
import time
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

BENCH_DIR = Path(__file__).resolve().parent
REPO_ROOT = BENCH_DIR.parent
sys.path.insert(0, str(BENCH_DIR))

import models as modelsmod  # noqa: E402
import providers  # noqa: E402
import publish  # noqa: E402
import report  # noqa: E402
import run  # noqa: E402


def _nyra_or_none():
    try:
        return run.find_nyra()
    except run.HarnessError:
        return None


NYRA = _nyra_or_none()
needs_nyra = unittest.skipUnless(NYRA, "the nyra compiler is not built (cargo build --release)")
NODE = shutil.which("node")
needs_node = unittest.skipUnless(NODE, "Node.js is not installed")


_RUST: dict = {}


def rust_toolchain():
    """A RustLang whose toolchain was found and works, or None (looked up once: it compiles a program)."""
    if "lang" not in _RUST:
        try:
            lang = run.RustLang(timeout=20)
            lang.preflight()
            _RUST["lang"] = lang
        except run.HarnessError:
            _RUST["lang"] = None
    return _RUST["lang"]


class NeedsRust(unittest.TestCase):
    """Base class: the tests of a subclass run only where a Rust toolchain works."""

    @classmethod
    def setUpClass(cls):
        if rust_toolchain() is None:
            raise unittest.SkipTest("no working Rust toolchain (rustc)")
        cls.lang = rust_toolchain()


def _task(expected="1\n"):
    return run.Task("t", "", "", expected, "0.1", "", "", Path("."))


def _run_main(*argv):
    out, err = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        code = run.main(list(argv))
    return code, out.getvalue(), err.getvalue()


class ExtractCode(unittest.TestCase):
    def test_tagged_and_untagged_blocks(self):
        self.assertEqual(run.extract_code("```nyra\nfn main() {}\n```"), "fn main() {}")
        self.assertEqual(run.extract_code("```\nprint(1)\n```"), "print(1)")

    def test_first_block_wins_and_prose_is_ignored(self):
        text = "Sure!\n```python\nprint(1)\n```\nand also\n```python\nprint(2)\n```\n"
        self.assertEqual(run.extract_code(text), "print(1)")

    def test_no_block_unterminated_or_empty(self):
        self.assertIsNone(run.extract_code("print(1)"))
        self.assertIsNone(run.extract_code("```python\nprint(1)\n"))
        self.assertIsNone(run.extract_code("```\n```"))
        self.assertIsNone(run.extract_code("```python\n   \n```"))
        self.assertIsNone(run.extract_code("use ```code``` inline"))

    def test_other_fence_styles(self):
        self.assertEqual(run.extract_code("~~~\nx = 1\n~~~"), "x = 1")
        self.assertEqual(run.extract_code("````\nprint('```')\n````"), "print('```')")
        self.assertEqual(run.extract_code("```python\r\nprint(1)\r\n```\r\n"), "print(1)")

    def test_indentation_inside_the_block_is_kept(self):
        self.assertEqual(run.extract_code("```py\nif x:\n    y = 1\n```"), "if x:\n    y = 1")


class Normalize(unittest.TestCase):
    def test_line_endings_and_trailing_whitespace(self):
        self.assertEqual(run.normalize_output("a  \r\nb\t\r\n\r\n\r\n"), "a\nb")
        self.assertEqual(run.normalize_output("a\rb"), "a\nb")

    def test_leading_whitespace_and_blank_lines_matter(self):
        self.assertNotEqual(run.normalize_output("  *\n"), run.normalize_output("*\n"))
        self.assertNotEqual(run.normalize_output("\nx\n"), run.normalize_output("x\n"))

    def test_first_diff_line(self):
        self.assertEqual(run.first_diff_line("a\nb\nc", "a\nx\nc"), 2)
        self.assertEqual(run.first_diff_line("a\nb", "a\nb\nc"), 3)
        self.assertEqual(run.first_diff_line("a\nb", ""), 1)

    def test_code_size(self):
        self.assertEqual(run.code_size("\n\nab\n\ncd\n"), (6, 2))


class Statistics(unittest.TestCase):
    def test_wilson(self):
        lo, hi = report.wilson(0, 10)
        self.assertEqual(lo, 0.0)
        self.assertAlmostEqual(hi, 0.2775, places=3)
        lo, hi = report.wilson(10, 10)
        self.assertAlmostEqual(lo, 0.7225, places=3)
        self.assertEqual(hi, 1.0)
        lo, hi = report.wilson(15, 19)
        self.assertTrue(0.56 < lo < 0.58 and 0.90 < hi < 0.92)
        self.assertEqual(report.wilson(0, 0), (0.0, 0.0))

    def test_mcnemar(self):
        self.assertEqual(report.mcnemar_exact(0, 0), 1.0)
        self.assertEqual(report.mcnemar_exact(3, 3), 1.0)
        self.assertAlmostEqual(report.mcnemar_exact(0, 5), 0.0625)
        self.assertAlmostEqual(report.mcnemar_exact(1, 5), 0.21875)

    def test_paired_sign_test(self):
        # one sample per task: differences are +-1 and the test is exactly McNemar's
        self.assertAlmostEqual(report.paired_sign_test([1, 1, 1, 1, 1, 0, 0]), report.mcnemar_exact(5, 0))
        self.assertAlmostEqual(report.paired_sign_test([1, -1, 0, 1, 1, 1, 1, -1]), report.mcnemar_exact(5, 2))
        self.assertEqual(report.paired_sign_test([0, 0, 0]), 1.0)
        self.assertEqual(report.paired_sign_test([]), 1.0)
        # fractional differences (several samples per task): seeded Monte Carlo, reproducible, and sensible
        mixed = [0.4, 0.2, 0.6, 0.2, 0.4, 0.2, 0.6, 0.4, 0.2, 0.2, 0.4, 0.6]
        p = report.paired_sign_test(mixed)
        self.assertEqual(p, report.paired_sign_test(mixed))
        self.assertLess(p, 0.01)  # twelve tasks, all in the same direction
        self.assertGreater(report.paired_sign_test([0.4, -0.4, 0.2, -0.2, 0.6, -0.6]), 0.5)

    def test_interval_width_follows_the_number_of_tasks_not_runs(self):
        lo5, hi5 = report.wilson_p(0.8, 5)
        lo50, hi50 = report.wilson_p(0.8, 50)
        self.assertGreater(hi5 - lo5, hi50 - lo50)
        recs = []
        for sample in range(4):  # 4 samples of the same 5 tasks
            for t in "abcde":
                recs.append(ReportSummary.rec(t, "python", t != "e", t != "e", kind="wrong_output", sample=sample))
        stats = report.summarize(recs, ["python"])["langs"]["python"]
        self.assertEqual((stats["n"], stats["tasks"], stats["pass_at_1"]), (20, 5, 16))
        self.assertEqual(stats["pass_at_1_ci"], list(report.wilson_p(0.8, 5)))

    def test_versions(self):
        self.assertEqual(run.parse_version("0.1"), (0, 1))
        self.assertEqual(run.parse_version("v0.3.2"), (0, 3))
        self.assertLess(run.parse_version("0.9"), run.parse_version("0.10"))
        with self.assertRaises(ValueError):
            run.parse_version("next")


class TaskSet(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tasks = run.load_tasks()

    def test_size_and_mix(self):
        self.assertTrue(70 <= len(self.tasks) <= 90, len(self.tasks))
        self.assertEqual({t.min_version for t in self.tasks}, {"0.1", "0.2", "0.3"})

    def test_hard_tier(self):
        # the tier that keeps first tries below the ceiling; reports show it as its own category
        hard = [t for t in self.tasks if t.category == "hard"]
        self.assertTrue(25 <= len(hard) <= 35, len(hard))
        for t in hard:
            self.assertEqual((t.difficulty, t.min_version), ("hard", "0.3"), t.id)
        self.assertEqual({t.id for t in self.tasks if t.difficulty == "hard"}, {t.id for t in hard})

    def test_every_category_is_documented(self):
        readme = (BENCH_DIR / "README.md").read_text(encoding="utf-8")
        for category in {t.category for t in self.tasks}:
            self.assertIn(f"| `{category}` |", readme, f"bench/README.md does not describe the category {category}")

    def test_every_task_has_a_python_reference_and_v01_tasks_a_nyra_one(self):
        for t in self.tasks:
            self.assertTrue((run.SOLUTIONS_DIR / "python" / f"{t.id}.py").is_file(), t.id)
            nyra = run.SOLUTIONS_DIR / "nyra" / f"{t.id}.nyra"
            if t.min_version == "0.1":
                self.assertTrue(nyra.is_file(), f"{t.id} needs a Nyra reference")

    def test_every_task_has_a_typescript_and_a_rust_reference(self):
        # unlike Nyra, these languages do not depend on a version: all tasks, always
        for t in self.tasks:
            self.assertTrue((run.SOLUTIONS_DIR / "typescript" / f"{t.id}.ts").is_file(), f"{t.id}: no TypeScript reference")
            self.assertTrue((run.SOLUTIONS_DIR / "rust" / f"{t.id}.rs").is_file(), f"{t.id}: no Rust reference")

    def test_no_orphan_solutions(self):
        ids = {t.id for t in self.tasks}
        for lang, ext in (("python", ".py"), ("nyra", ".nyra"), ("typescript", ".ts"), ("rust", ".rs")):
            for path in (run.SOLUTIONS_DIR / lang).glob(f"*{ext}"):
                self.assertIn(path.stem, ids, f"{path} has no task")

    def test_reference_solutions_have_the_shape_the_mock_provider_needs(self):
        # the mock's deliberately broken replies edit these programs textually
        for t in self.tasks:
            nyra = run.SOLUTIONS_DIR / "nyra" / f"{t.id}.nyra"
            if nyra.is_file():
                self.assertIn("fn main() {", nyra.read_text(encoding="utf-8"), nyra.name)
            rust = (run.SOLUTIONS_DIR / "rust" / f"{t.id}.rs").read_text(encoding="utf-8")
            self.assertIn("fn main() {", rust, f"{t.id}.rs")
            for lang, ext in (("typescript", ".ts"), ("rust", ".rs")):
                text = (run.SOLUTIONS_DIR / lang / f"{t.id}{ext}").read_text(encoding="utf-8")
                self.assertTrue(text.endswith("\n") and "\r" not in text, f"{t.id}{ext}")

    def test_solution_files_are_ascii(self):
        for lang, ext in (("python", ".py"), ("nyra", ".nyra"), ("typescript", ".ts"), ("rust", ".rs")):
            for path in (run.SOLUTIONS_DIR / lang).glob(f"*{ext}"):
                self.assertTrue(path.read_text(encoding="utf-8").isascii(), path.name)

    def test_expected_outputs_are_clean(self):
        for t in self.tasks:
            self.assertTrue(t.expected_output.endswith("\n"), t.id)
            self.assertNotIn("\r", t.expected_output, t.id)
            self.assertEqual(run.normalize_output(t.expected_output) + "\n", t.expected_output, t.id)
            self.assertTrue(t.prompt.strip() and t.title.strip() and t.category, t.id)

    def test_prompts_never_mention_a_language(self):
        names = re.compile(r"\b(python|nyra|typescript|javascript|node(\.?js)?|rust|rustc|cargo)\b")
        for t in self.tasks:
            self.assertIsNone(names.search(t.prompt.lower()), t.id)
            self.assertIsNone(names.search(t.title.lower()), t.id)

    def test_digest_is_stable(self):
        self.assertEqual(run.tasks_digest(), run.tasks_digest())


class RunLimited(unittest.TestCase):
    def _run(self, code, timeout=10, max_output=run.MAX_OUTPUT_BYTES):
        with run.scratch_dir() as wd:
            env = run.child_env(wd)
            return run.run_limited([sys.executable, "-c", code], cwd=wd, env=env, timeout=timeout,
                                   max_output=max_output)

    def test_normal_exit(self):
        p = self._run("print('hi')")
        self.assertEqual((p.returncode, p.stdout.strip(), p.timed_out, p.truncated), (0, b"hi", False, False))

    def test_timeout_kills_the_process(self):
        p = self._run("while True: pass", timeout=1)
        self.assertTrue(p.timed_out)
        self.assertLess(p.elapsed, 5)

    def test_runaway_output_is_capped_and_killed(self):
        p = self._run("while True: print('x' * 100)", timeout=20, max_output=10_000)
        self.assertTrue(p.truncated)
        self.assertFalse(p.timed_out)
        self.assertLessEqual(len(p.stdout), 10_000)

    def test_missing_program_is_reported_not_raised(self):
        with run.scratch_dir() as wd:
            p = run.run_limited(["definitely-not-a-program-xyz"], cwd=wd, env=run.child_env(wd), timeout=5)
        self.assertIsNotNone(p.spawn_error)

    def test_secrets_are_not_passed_to_programs(self):
        with mock.patch.dict(os.environ, {"ANTHROPIC_API_KEY": "sk-secret", "MY_PASSWORD": "x", "HARMLESS": "ok"}):
            p = self._run("import os; print(os.environ.get('ANTHROPIC_API_KEY'), os.environ.get('MY_PASSWORD'),"
                          " os.environ.get('HARMLESS'))")
        self.assertEqual(p.stdout.decode().split(), ["None", "None", "ok"])

    def test_temp_dir_is_private(self):
        with run.scratch_dir() as wd:
            env = run.child_env(wd)
            p = run.run_limited([sys.executable, "-c", "import tempfile; print(tempfile.gettempdir())"], cwd=wd, env=env,
                                timeout=10)
            self.assertEqual(Path(p.stdout.decode().strip()).resolve(), wd.resolve())

    def test_scrub_paths(self):
        with run.scratch_dir() as wd:
            text = f'File "{wd}{os.sep}main.py", line 1\n  at {wd}'
            self.assertEqual(run.scrub_paths(text, wd), 'File "main.py", line 1\n  at .')


class EvaluatePython(unittest.TestCase):
    def setUp(self):
        self.lang = run.PythonLang(timeout=3)

    def test_pass_and_output_normalization(self):
        r = self.lang.evaluate("print(1)\nprint('x   ')", _task("1\nx\n\n"))
        self.assertTrue(r.passed, r)

    def test_wrong_output_feedback_hides_the_expected_output(self):
        r = self.lang.evaluate("print(41)", _task("42\n"))
        self.assertEqual(r.kind, "wrong_output")
        self.assertIn("41", r.feedback)
        self.assertNotIn("42", r.feedback)
        self.assertIn("line 1", r.feedback)

    def test_nonzero_exit_fails_even_if_the_output_matches(self):
        r = self.lang.evaluate("print(1)\nimport sys\nsys.exit(2)", _task("1\n"))
        self.assertEqual((r.passed, r.kind, r.exit_code), (False, "runtime_error", 2))

    def test_traceback_is_returned_without_the_temp_path(self):
        r = self.lang.evaluate("print(1)\nraise ValueError('boom')", _task("1\n"))
        self.assertEqual(r.kind, "runtime_error")
        self.assertIn("ValueError: boom", r.feedback)
        self.assertNotIn("nyra-bench-", r.feedback)
        self.assertIn('File "main.py"', r.feedback)

    def test_syntax_error_is_a_compile_error(self):
        r = self.lang.evaluate("def (:\n", _task())
        self.assertEqual(r.kind, "compile_error")
        self.assertNotIn("nyra-bench-", r.feedback)

    def test_timeout_and_output_limit(self):
        self.assertEqual(self.lang.evaluate("while True: pass", _task()).kind, "timeout")
        self.assertEqual(self.lang.evaluate("while True: print(1)", _task()).kind, "output_limit")

    def test_only_the_standard_library_is_importable_in_isolated_mode(self):
        r = self.lang.evaluate("import json, math, itertools\nprint(1)", _task())
        self.assertTrue(r.passed)


@needs_nyra
class EvaluateNyra(unittest.TestCase):
    HELLO = "fn main() {\n    print(42)\n}\n"

    def test_both_backends_pass(self):
        for backend in ("native", "js"):
            r = run.NyraLang(NYRA, backend=backend, timeout=10).evaluate(self.HELLO, _task("42\n"))
            self.assertTrue(r.passed, (backend, r))

    def test_compiler_errors_come_back_as_json(self):
        lang = run.NyraLang(NYRA, timeout=10)
        r = lang.evaluate("fn main() {\n    print(cout)\n}\n", _task())
        self.assertEqual(r.kind, "compile_error")
        self.assertEqual(r.errors[0]["code"], "E0201")
        self.assertEqual(r.errors[0]["file"], "main.nyra")  # relative: no temp path leaks into the prompt
        self.assertIn('"code":"E0201"', r.feedback)

    def test_wrong_output_timeout_and_output_limit(self):
        lang = run.NyraLang(NYRA, timeout=2)
        self.assertEqual(lang.evaluate(self.HELLO, _task("43\n")).kind, "wrong_output")
        self.assertEqual(lang.evaluate("fn main() {\n    while true { }\n}\n", _task()).kind, "timeout")
        self.assertEqual(lang.evaluate("fn main() {\n    while true { print(1) }\n}\n", _task()).kind, "output_limit")

    def test_runtime_crash(self):
        r = run.NyraLang(NYRA, timeout=5).evaluate("fn main() {\n    let z = 0\n    print(1 / z)\n}\n", _task())
        self.assertEqual(r.kind, "runtime_error")

    def test_parallel_evaluations_do_not_share_files(self):
        lang = run.NyraLang(NYRA, timeout=10)
        progs = [f"fn main() {{\n    print({n})\n}}\n" for n in range(6)]
        results = [None] * len(progs)

        def work(i):
            results[i] = lang.evaluate(progs[i], _task(f"{i}\n")).passed

        threads = [threading.Thread(target=work, args=(i,)) for i in range(len(progs))]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        self.assertEqual(results, [True] * len(progs))


class ReferenceSolutions(unittest.TestCase):
    """The expected outputs must be what the Python references print (the verifier does the full check
    including Nyra on both backends; this keeps `unittest` honest when the compiler is absent)."""

    def test_python_references_match_expected_output(self):
        lang = run.PythonLang(timeout=20)
        for t in run.load_tasks():
            with self.subTest(task=t.id):
                r = lang.evaluate(lang.reference_code(t.id), t)
                self.assertTrue(r.passed, (r.kind, r.stderr[-300:]))


@needs_nyra
class MockPipeline(unittest.TestCase):
    TASKS = "fizzbuzz,gcd_pairs,grade_letters"

    def _run(self, *extra):
        # Nyra and Python keep these tests quick; the four-language runs have their own tests below.
        langs = [] if "--langs" in extra else ["--langs", "nyra,python"]
        with tempfile.TemporaryDirectory() as out:
            code, stdout, stderr = _run_main("--provider", "mock", "--tasks", self.TASKS, "--out", out, "-q", *langs,
                                             *extra)
            files = sorted(p.name for p in Path(out).iterdir())
            results = json.loads(next(Path(out).glob("*.json")).read_text(encoding="utf-8"))
            latest = (Path(out) / "latest.md").read_text(encoding="utf-8")
        return code, stdout, stderr, results, files, latest

    def test_all_pass_in_both_languages(self):
        code, stdout, stderr, results, files, latest = self._run()
        self.assertEqual(code, 0, stderr)
        self.assertEqual(len(results["records"]), 6)
        self.assertTrue(all(r["status"] == "pass" and r["first_try"] and r["attempts_used"] == 1
                            for r in results["records"]))
        self.assertEqual(files[0][:11], "2" + files[0][1:11])  # dated file name
        self.assertIn("latest.md", files)
        self.assertIn("MOCK RUN", latest)
        self.assertEqual(results["summary"]["langs"]["nyra"]["pass_at_1"], 3)
        self.assertTrue(results["run"]["mock"] and results["run"]["complete"])

    def test_result_records_are_complete(self):
        _, _, _, results, _, _ = self._run()
        rec = next(r for r in results["records"] if r["lang"] == "nyra")
        a = rec["attempts"][0]
        for key in ("reply", "code", "usage", "chars", "lines", "code_tokens", "result", "latency_s", "served_model"):
            self.assertIn(key, a)
        self.assertIn("nyra_spec", results["run"]["system_prompts"]["nyra"])
        self.assertNotIn("nyra_spec", results["run"]["system_prompts"]["python"])
        self.assertEqual(len(results["run"]["spec"]["sha256"]), 64)

    def test_result_files_are_self_contained_and_carry_no_local_paths(self):
        _, _, _, results, _, _ = self._run()
        run_meta = results["run"]
        by_id = {t.id: t for t in run.load_tasks()}
        self.assertEqual(run_meta["tasks"]["fizzbuzz"]["prompt"], by_id["fizzbuzz"].prompt)
        self.assertEqual(run_meta["served_models"], ["mock"])
        for path in (run_meta["nyra"]["path"], run_meta["spec"]["path"]):
            self.assertFalse(Path(path).is_absolute(), path)
        self.assertEqual(run_meta["spec"]["path"], "docs/SPEC.md")
        self.assertNotIn(os.path.expanduser("~"), json.dumps(run_meta))

    def test_a_run_where_nothing_finishes_writes_no_files_and_stops(self):
        err = Exception("invalid x-api-key")
        err.status_code = 401
        with tempfile.TemporaryDirectory() as out, \
                mock.patch.object(providers.AnthropicProvider, "_make_client", staticmethod(lambda: FakeClient([err] * 10))):
            code, _, stderr = _run_main("--provider", "anthropic", "--tasks", "fizzbuzz", "--langs", "python",
                                        "--out", out, "-q", "--jobs", "1")
            self.assertEqual(list(Path(out).iterdir()), [])
        self.assertEqual(code, 2)
        self.assertIn("no result files were written", stderr)
        self.assertIn("HTTP 401", stderr)

    def test_missing_api_key_stops_before_any_work(self):
        with tempfile.TemporaryDirectory() as out, mock.patch.dict(os.environ):
            os.environ.pop("ANTHROPIC_API_KEY", None)
            code, _, stderr = _run_main("--provider", "anthropic", "--tasks", "fizzbuzz", "--langs", "python",
                                        "--out", out)
            self.assertEqual(list(Path(out).iterdir()), [])
        self.assertEqual(code, 2)
        self.assertIn("ANTHROPIC_API_KEY", stderr)

    def test_dry_run_needs_no_key(self):
        with tempfile.TemporaryDirectory() as out, mock.patch.dict(os.environ):
            os.environ.pop("ANTHROPIC_API_KEY", None)
            code, stdout, _ = _run_main("--provider", "anthropic", "--tasks", "fizzbuzz", "--langs", "python",
                                        "--out", out, "--dry-run")
            self.assertEqual(list(Path(out).iterdir()), [])
        self.assertEqual(code, 0)
        self.assertIn("provider=anthropic model=claude-opus-5-5", stdout)
        self.assertIn("at most 4 model calls", stdout)

    def test_each_failure_kind_is_repaired(self):
        expected_kind = {"no_code": "no_code", "syntax": "compile_error", "runtime": None, "wrong": "wrong_output"}
        for defect, kind in expected_kind.items():
            with self.subTest(defect=defect):
                code, _, stderr, results, _, _ = self._run("--mock-flaky", defect)
                self.assertEqual(code, 0, stderr)
                for rec in results["records"]:
                    self.assertEqual(rec["status"], "pass")
                    self.assertEqual(rec["attempts_used"], 2)
                    self.assertFalse(rec["first_try"])
                    first = rec["attempts"][0]
                    self.assertFalse(first["result"]["passed"])
                    self.assertTrue(first["feedback"])
                    if kind:
                        self.assertEqual(first["result"]["kind"], kind)
                self.assertEqual(results["summary"]["langs"]["python"]["pass_at_1"], 0)
                self.assertEqual(results["summary"]["langs"]["python"]["pass_within_repairs"], 3)

    def test_no_repairs_means_failure_and_exit_code_1(self):
        code, _, stderr, results, _, latest = self._run("--mock-flaky", "wrong", "--repairs", "0")
        self.assertEqual(code, 1)
        self.assertTrue(all(r["status"] == "fail" and r["attempts_used"] == 1 for r in results["records"]))
        self.assertIn("FAIL (wrong_output)", latest)

    def test_repair_conversation_replays_the_reply_and_the_feedback(self):
        seen = []
        real = providers.MockProvider.complete

        def spy(self, system, messages, meta):
            seen.append((meta["attempt"], [m["role"] for m in messages], [m["content"] for m in messages]))
            return real(self, system, messages, meta)

        with mock.patch.object(providers.MockProvider, "complete", spy):
            self._run("--mock-flaky", "wrong", "--jobs", "1")
        second = [s for s in seen if s[0] == 2][0]
        self.assertEqual(second[1], ["user", "assistant", "user"])
        self.assertIn("does not match", second[2][2])

    def test_samples_repeat_every_task(self):
        _, _, _, results, _, _ = self._run("--samples", "2")
        self.assertEqual(len(results["records"]), 12)
        self.assertEqual({r["sample"] for r in results["records"]}, {0, 1})

    def test_python_only_run_has_no_nyra_dependency(self):
        _, _, _, results, _, latest = self._run("--langs", "python")
        self.assertEqual(results["run"]["langs"], ["python"])
        self.assertIsNone(results["run"]["nyra"])
        self.assertNotIn("Nyra</", latest)

    def test_dry_run_writes_nothing(self):
        with tempfile.TemporaryDirectory() as out:
            code, stdout, _ = _run_main("--provider", "mock", "--tasks", "fizzbuzz", "--langs", "nyra,python",
                                        "--out", out, "--dry-run")
            self.assertEqual(code, 0)
            self.assertEqual(list(Path(out).iterdir()), [])
            self.assertIn("at most 8 model calls", stdout)
            _, stdout, _ = _run_main("--provider", "mock", "--tasks", "fizzbuzz", "--out", out, "--dry-run")
            self.assertIn("1 tasks x 4 languages", stdout)  # the default is all four
            self.assertIn("at most 16 model calls", stdout)

    def test_task_selection_errors(self):
        code, _, stderr = _run_main("--provider", "mock", "--tasks", "no_such_task", "--dry-run")
        self.assertEqual(code, 2)
        self.assertIn("no task matches", stderr)

    def test_max_version_filters_every_language_and_unknown_flags_fail_cleanly(self):
        with tempfile.TemporaryDirectory() as out:
            _, stdout, _ = _run_main("--provider", "mock", "--max-version", "0.2", "--langs", "python", "--out", out,
                                     "--dry-run")
        self.assertIn("0.2  multiplication_table", stdout)
        self.assertNotIn("bubble_sort", stdout.split("skipped")[0])

    def test_result_files_are_never_overwritten(self):
        with tempfile.TemporaryDirectory() as out:
            a, _ = run.result_paths(Path(out), "2026-10-07", "mock", "mock")
            a.write_text("{}")
            b, _ = run.result_paths(Path(out), "2026-10-07", "mock", "mock")
            self.assertNotEqual(a, b)
            self.assertEqual(run.result_paths(Path(out), "2026-10-07", "anthropic", "a/b c")[0].name,
                             "2026-10-07-anthropic-a-b-c.json")


class VerifyTool(unittest.TestCase):
    """verify.py must be able to fail: each defect below has to be reported."""

    def setUp(self):
        import verify
        self.verify = verify
        self.tmp = tempfile.TemporaryDirectory()
        self.sol = Path(self.tmp.name)
        for lang in ("python", "nyra"):
            (self.sol / lang).mkdir()
        patcher = mock.patch.object(run, "SOLUTIONS_DIR", self.sol)
        patcher.start()
        self.addCleanup(patcher.stop)
        self.addCleanup(self.tmp.cleanup)
        self.args = SimpleNamespace(timeout=10, write=False, strict=False)

    def task(self, expected="1\n", version="0.1"):
        return run.Task("t", "T", "prompt", expected, version, "math", "easy", Path("t.json"))

    def check(self, task, nyra_langs=None, version=None):
        return self.verify.check_task(task, self.args, nyra_langs or {}, version)

    def test_good_task(self):
        (self.sol / "python" / "t.py").write_text("print(1)\n")
        res = self.check(self.task())
        self.assertEqual((res["problems"], res["new_expected"]), ([], None))

    def test_missing_python_reference(self):
        self.assertIn("missing", self.check(self.task())["problems"][0])

    def test_expected_output_that_disagrees_with_the_reference(self):
        (self.sol / "python" / "t.py").write_text("print(1)\n")
        res = self.check(self.task(expected="2\n"))
        self.assertIn("differs", res["problems"][0])
        self.args.write = True
        res = self.check(self.task(expected="2\n"))
        self.assertEqual((res["problems"], res["new_expected"]), ([], "1\n"))

    def test_nondeterministic_reference_is_rejected(self):
        (self.sol / "python" / "t.py").write_text("import random\nprint(random.random())\n")
        self.assertTrue(any("not deterministic" in p for p in self.check(self.task(expected="0.5\n"))["problems"]))

    def test_crashing_reference_is_rejected(self):
        (self.sol / "python" / "t.py").write_text("raise SystemExit(3)\n")
        self.assertIn("failed", self.check(self.task())["problems"][0])

    def test_expected_output_lint(self):
        (self.sol / "python" / "t.py").write_text("print('a ')\n")
        self.assertTrue(any("trailing whitespace" in p for p in self.check(self.task(expected="a \n"))["problems"]))

    @needs_nyra
    def test_nyra_reference_must_match_on_every_backend(self):
        (self.sol / "python" / "t.py").write_text("print(42)\n")
        langs = {b: run.NyraLang(NYRA, backend=b, timeout=10) for b in ("native", "js")}
        good = "fn main() {\n    print(42)\n}\n"
        (self.sol / "nyra" / "t.nyra").write_text(good)
        self.assertEqual(self.check(self.task("42\n"), langs, (0, 1))["problems"], [])
        (self.sol / "nyra" / "t.nyra").write_text(good.replace("42", "41"))
        problems = self.check(self.task("42\n"), langs, (0, 1))["problems"]
        self.assertEqual(len(problems), 2)  # once per backend
        self.assertIn("wrong_output", problems[0])
        (self.sol / "nyra" / "t.nyra").write_text("fn main() {\n    print(oops)\n}\n")
        self.assertIn("compile_error", self.check(self.task("42\n"), langs, (0, 1))["problems"][0])

    @needs_nyra
    def test_missing_and_premature_nyra_references(self):
        (self.sol / "python" / "t.py").write_text("print(42)\n")
        langs = {"native": run.NyraLang(NYRA, backend="native", timeout=10)}
        res = self.check(self.task("42\n"), langs, (0, 1))  # supported version, no reference
        self.assertEqual(res["problems"], [])
        self.assertTrue(any("no Nyra reference" in n for n in res["notes"]))
        self.args.strict = True
        self.assertTrue(self.check(self.task("42\n"), langs, (0, 1))["problems"])
        self.args.strict = False
        res = self.check(self.task("42\n", version="0.3"), langs, (0, 1))  # not supported yet: only a note
        self.assertEqual(res["problems"], [])
        (self.sol / "nyra" / "t.nyra").write_text("fn main() {\n    print(42)\n}\n")
        self.assertIn("min_version", self.check(self.task("42\n", version="0.3"), langs, (0, 1))["problems"][0])


class FakeClient:
    """Stands in for anthropic.Anthropic: records requests, replies from a script."""

    def __init__(self, replies=None, count_error=None):
        self.requests, self.count_requests = [], []
        self.replies = list(replies or [])
        self.count_error = count_error
        self.messages = self

    def create(self, **kwargs):
        self.requests.append(kwargs)
        item = self.replies.pop(0) if self.replies else None
        if isinstance(item, Exception):
            raise item
        return item or SimpleNamespace(
            content=[SimpleNamespace(type="thinking", thinking="", signature="s"),
                     SimpleNamespace(type="text", text="```python\nprint(1)\n```")],
            usage=SimpleNamespace(input_tokens=100, output_tokens=40, cache_creation_input_tokens=5,
                                  cache_read_input_tokens=7),
            stop_reason="end_turn", _request_id="req_1")

    def count_tokens(self, **kwargs):
        self.count_requests.append(kwargs)
        if self.count_error:
            raise self.count_error
        text = kwargs["messages"][0]["content"]
        return SimpleNamespace(input_tokens=8 + len(text))


class AnthropicProviderWithFakeClient(unittest.TestCase):
    def test_request_shape_and_reply_parsing(self):
        client = FakeClient()
        p = providers.AnthropicProvider(client=client, extra={"thinking": {"type": "adaptive"}, "foo": 1},
                                        effort="low", max_tokens=1234)
        reply = p.complete("SYS", [{"role": "user", "content": "task"}], {"task_id": "x", "lang": "python", "attempt": 1})
        req = client.requests[0]
        self.assertEqual(req["model"], "claude-opus-5-5")
        self.assertEqual((req["max_tokens"], req["system"]), (1234, "SYS"))
        self.assertEqual(req["messages"], [{"role": "user", "content": "task"}])
        self.assertEqual(req["output_config"], {"effort": "low"})
        self.assertEqual(req["thinking"], {"type": "adaptive"})
        self.assertEqual(req["extra_body"], {"foo": 1})
        self.assertNotIn("temperature", req)
        self.assertEqual(reply.text, "```python\nprint(1)\n```")  # the thinking block is not part of the text
        self.assertEqual((reply.usage.input_tokens, reply.usage.output_tokens), (112, 40))
        self.assertFalse(reply.usage.estimated)
        self.assertEqual((reply.stop_reason, reply.request_id), ("end_turn", "req_1"))

    def test_nothing_is_sent_that_was_not_asked_for(self):
        client = FakeClient()
        providers.AnthropicProvider(client=client).complete("S", [{"role": "user", "content": "t"}], {})
        self.assertEqual(sorted(client.requests[0]), ["max_tokens", "messages", "model", "system"])

    def test_error_mapping(self):
        def err(status):
            e = Exception("nope")
            e.status_code = status
            return e

        for status, fatal in ((400, True), (401, True), (403, True), (404, True), (429, False), (500, False),
                              (529, False), (None, False)):
            client = FakeClient([err(status)])
            with self.assertRaises(providers.ProviderError) as cm:
                providers.AnthropicProvider(client=client).complete("S", [{"role": "user", "content": "t"}], {})
            self.assertEqual(cm.exception.fatal, fatal, status)
        # errors raised by the SDK before anything is sent would repeat on every call: stop the run
        for exc in (TypeError("unexpected keyword argument 'output_config'"),
                    ValueError("Streaming is required for operations that may take longer than 10 minutes")):
            with self.assertRaises(providers.ProviderError) as cm:
                providers.AnthropicProvider(client=FakeClient([exc])).complete("S", [{"role": "user", "content": "t"}], {})
            self.assertTrue(cm.exception.fatal, exc)

    def test_missing_key_is_fatal_and_names_the_variable(self):
        with mock.patch.dict(os.environ, {}, clear=False):
            os.environ.pop("ANTHROPIC_API_KEY", None)
            provider = providers.AnthropicProvider()  # constructing needs neither key nor SDK
            with self.assertRaises(providers.ProviderError) as cm:
                provider.ensure_ready()
        self.assertTrue(cm.exception.fatal)
        self.assertIn("ANTHROPIC_API_KEY", str(cm.exception))

    def test_count_tokens_subtracts_the_per_message_overhead(self):
        client = FakeClient()
        p = providers.AnthropicProvider(client=client)
        self.assertEqual(p.count_tokens("hello"), 5)  # (8 + 5) - (8 + 1 - 1)
        self.assertEqual(p.count_tokens("x"), 1)
        self.assertEqual(client.count_requests[0]["model"], "claude-opus-5-5")

    def test_count_tokens_failure_disables_counting_without_raising(self):
        client = FakeClient(count_error=RuntimeError("no"))
        p = providers.AnthropicProvider(client=client)
        with contextlib.redirect_stderr(io.StringIO()):
            self.assertIsNone(p.count_tokens("hello"))
            self.assertIsNone(p.count_tokens("again"))
        self.assertEqual(len(client.count_requests), 1)

    def test_run_one_with_the_fake_client_end_to_end(self):
        client = FakeClient([SimpleNamespace(
            content=[SimpleNamespace(type="text", text="```python\nprint(1)\n```")],
            usage=SimpleNamespace(input_tokens=10, output_tokens=5), stop_reason="end_turn")])
        ctx = run.RunContext(provider=providers.AnthropicProvider(client=client), repairs=2, count_tokens=True)
        rec = run.run_one(_task("1\n"), run.PythonLang(timeout=5), 0, ctx)
        self.assertEqual((rec["status"], rec["first_try"], rec["attempts_used"]), ("pass", True, 1))
        a = rec["attempts"][0]
        self.assertEqual((a["usage"]["output_tokens"], a["code_tokens"], a["chars"], a["lines"]), (5, 8, 8, 1))

    def test_provider_error_marks_the_run_and_fatal_errors_abort(self):
        e = Exception("bad key")
        e.status_code = 401
        ctx = run.RunContext(provider=providers.AnthropicProvider(client=FakeClient([e])), repairs=1,
                             count_tokens=False)
        rec = run.run_one(_task(), run.PythonLang(timeout=5), 0, ctx)
        self.assertEqual(rec["status"], "error")
        self.assertTrue(ctx.abort.is_set() and ctx.fatal)


class _StubAnthropicServer(http.server.ThreadingHTTPServer):
    def __init__(self):
        super().__init__(("127.0.0.1", 0), _StubHandler)
        self.log = []


class _StubHandler(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get("content-length", 0))) or b"{}")
        self.server.log.append((self.path, body, {k.lower(): v for k, v in self.headers.items()}))
        if "count_tokens" in self.path:
            payload = {"input_tokens": 10 + len(body["messages"][0]["content"])}
        else:
            payload = {"id": "msg_1", "type": "message", "role": "assistant", "model": body.get("model", "m"),
                       "content": [{"type": "thinking", "thinking": "", "signature": "abc"},
                                   {"type": "text", "text": "```python\nprint(1)\n```"}],
                       "stop_reason": "end_turn", "stop_sequence": None,
                       "usage": {"input_tokens": 120, "output_tokens": 60}}
        data = json.dumps(payload).encode()
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, *args):
        pass


try:
    import anthropic  # noqa: F401
    HAVE_SDK = True
except ImportError:
    HAVE_SDK = False


@unittest.skipUnless(HAVE_SDK, "the anthropic SDK is not installed (pip install -r bench/requirements.txt)")
class AnthropicProviderWithRealSdk(unittest.TestCase):
    """The real SDK talking to a local stub server: checks the wire format without any real API call."""

    def test_round_trip(self):
        server = _StubAnthropicServer()
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            env = {"ANTHROPIC_API_KEY": "test-key-not-real", "ANTHROPIC_BASE_URL": f"http://127.0.0.1:{server.server_port}"}
            with mock.patch.dict(os.environ, env):
                p = providers.AnthropicProvider(effort="medium", extra={"thinking": {"type": "adaptive"}, "foo": 7})
                reply = p.complete("SYS", [{"role": "user", "content": "task"}], {})
                n = p.count_tokens("print(1)")
        finally:
            server.shutdown()
            server.server_close()
        self.assertEqual(reply.text, "```python\nprint(1)\n```")
        self.assertEqual((reply.usage.input_tokens, reply.usage.output_tokens), (120, 60))
        path, body, headers = server.log[0]
        self.assertTrue(path.startswith("/v1/messages"))
        self.assertEqual(body["model"], "claude-opus-5-5")
        self.assertEqual(body["system"], "SYS")
        self.assertEqual(body["output_config"], {"effort": "medium"})
        self.assertEqual(body["thinking"], {"type": "adaptive"})
        self.assertEqual(body["foo"], 7)  # unknown fields travel in extra_body
        self.assertNotIn("temperature", body)
        self.assertEqual(headers["x-api-key"], "test-key-not-real")
        self.assertEqual(n, len("print(1)"))  # (10 + 8) - (10 + 1 - 1)


class ReportSummary(unittest.TestCase):
    @staticmethod
    def rec(task, lang, first, passed, kind="pass", out=100, code_tokens=50, status=None, attempts=None, sample=0,
            errors=()):
        ok = first
        first_attempt = {"n": 1, "usage": {"input_tokens": 1000, "output_tokens": out}, "code_tokens": code_tokens,
                         "chars": 200, "lines": 10,
                         "result": {"passed": ok, "kind": "pass" if ok else kind, "errors": list(errors)}}
        atts = [first_attempt]
        for n in range(2, (attempts or 1) + 1):
            atts.append({"n": n, "usage": {"input_tokens": 1500, "output_tokens": out},
                         "result": {"passed": passed and n == attempts, "kind": "pass" if passed and n == attempts else kind,
                                    "errors": []}})
        return {"task_id": task, "lang": lang, "sample": sample, "status": status or ("pass" if passed else "fail"),
                "first_try": first, "attempts_used": len(atts), "attempts": atts, "error": None}

    def test_common_set_pairing_and_exclusions(self):
        recs = [
            self.rec("a", "nyra", True, True), self.rec("a", "python", True, True),
            self.rec("b", "nyra", False, True, kind="compile_error", attempts=2, errors=[{"code": "E0201"}]),
            self.rec("b", "python", True, True),
            self.rec("c", "nyra", False, False, kind="wrong_output", attempts=2), self.rec("c", "python", False, True,
                                                                                         kind="runtime_error", attempts=2),
            self.rec("d", "nyra", True, True), self.rec("d", "python", True, True, status="error"),
        ]
        s = report.summarize(recs, ["nyra", "python"])
        self.assertEqual((s["runs"], s["excluded_runs"]), (3, 1))  # task d is dropped for both languages
        nyra, py = s["langs"]["nyra"], s["langs"]["python"]
        self.assertEqual((nyra["pass_at_1"], nyra["pass_within_repairs"]), (1, 2))
        self.assertEqual((py["pass_at_1"], py["pass_within_repairs"]), (2, 3))
        self.assertEqual(nyra["first_attempt_failures"], {"compile_error": 1, "wrong_output": 1})
        self.assertEqual(nyra["first_attempt_error_codes"], {"E0201": 1})
        self.assertAlmostEqual(nyra["avg_attempts"], (1 + 2 + 2) / 3)
        self.assertEqual(nyra["avg_output_tokens_first_attempt"], 100)
        self.assertEqual(s["paired"]["n"], 1)
        self.assertEqual(s["paired"]["baseline"], "nyra")
        ft = s["paired"]["pairs"]["python"]["first_try"]
        self.assertEqual((ft["both"], ft["only_a"], ft["only_b"], ft["neither"]), (1, 0, 1, 1))

    def test_render_markdown_smoke(self):
        recs = [self.rec("a", "nyra", True, True), self.rec("a", "python", False, True, kind="wrong_output", attempts=2)]
        results = {"run": {"date": "2026-10-07", "provider": {"name": "anthropic", "model": "m"}, "langs": ["nyra", "python"],
                           "repairs": 3, "samples": 1, "timeout_s": 10, "backend": "native", "mock": False,
                           "complete": True, "nyra": {"version": "nyra 0.1.0"}, "task_ids": ["a"],
                           "excluded_tasks": [{"id": "z", "reason": "needs Nyra 0.2 (limit is 0.1)"}]},
                   "records": recs, "summary": report.summarize(recs, ["nyra", "python"])}
        md = report.render_markdown(results, {})
        self.assertIn("**pass@1**", md)
        self.assertIn("100% (1/1)", md)
        self.assertIn("pass (attempt 2)", md)
        self.assertNotIn("MOCK", md)
        self.assertTrue(md.isascii())

    def test_abnormal_stop_reasons_are_reported(self):
        a = self.rec("a", "nyra", False, True, kind="no_code", attempts=2)
        a["attempts"][0]["stop_reason"] = "max_tokens"
        recs = [a, self.rec("a", "python", True, True)]
        s = report.summarize(recs, ["nyra", "python"])
        self.assertEqual(s["langs"]["nyra"]["odd_stop_reasons"], {"max_tokens": 1})
        results = {"run": {"date": "d", "provider": {"name": "p", "model": "m"}, "langs": ["nyra", "python"], "repairs": 3,
                           "samples": 1, "timeout_s": 10, "mock": False, "complete": True, "task_ids": ["a"]},
                   "records": recs, "summary": s}
        self.assertIn("max_tokens x1", report.render_markdown(results, {}))

    def test_empty_result_does_not_crash(self):
        results = {"run": {"date": "d", "provider": {"name": "p", "model": "m"}, "langs": ["python"], "repairs": 1,
                           "samples": 1, "timeout_s": 10, "mock": False, "complete": True, "task_ids": []},
                   "records": [], "summary": report.summarize([], ["python"])}
        self.assertIn("No complete results", report.render_markdown(results, {}))

# ===================================================================== TypeScript and Rust


class NodeOutputHelpers(unittest.TestCase):
    """What the model is shown when a TypeScript program fails: Node's output without Node's own noise."""

    SYNTAX = (
        "C:\\Users\\x\\Temp\\nyra-bench-abc\\main.ts:1\n"
        "const x: number = ;\n"
        "                  ^\n"
        "\n"
        "SyntaxError [ERR_INVALID_TYPESCRIPT_SYNTAX]: Expression expected\n"
        "    at parseTypeScript (node:internal/modules/typescript:72:36)\n"
        "    at processTypeScriptCode (node:internal/modules/typescript:146:42)\n"
        "    at Module._compile (node:internal/modules/cjs/loader:1712:15)\n"
        "    at Module.executeUserEntryPoint [as runMain] (node:internal/modules/run_main:154:5) {\n"
        "  code: 'ERR_INVALID_TYPESCRIPT_SYNTAX'\n"
        "}\n"
        "\n"
        "Node.js v25.2.1\n")
    CRASH = (
        "main.ts:3\n"
        "function boom()       { throw new Error(\"kaboom\"); }\n"
        "                        ^\n"
        "\n"
        "Error: kaboom\n"
        "    at boom (main.ts:3:31)\n"
        "    at Object.<anonymous> (main.ts:4:1)\n"
        "    at Module._compile (node:internal/modules/cjs/loader:1760:14)\n"
        "    at node:internal/main/run_main_module:33:47\n"
        "\n"
        "Node.js v25.2.1\n")
    RUNTIME_SYNTAX_ERROR = (
        "undefined:1\n"
        "x\n"
        "^\n"
        "\n"
        "SyntaxError: Unexpected token 'x', \"x\" is not valid JSON\n"
        "    at JSON.parse (<anonymous>)\n"
        "    at Object.<anonymous> (main.ts:2:6)\n"
        "    at Module._compile (node:internal/modules/cjs/loader:1760:14)\n")

    def test_internal_frames_the_version_line_and_the_code_tail_are_removed(self):
        cleaned = run.clean_node_stderr(self.SYNTAX)
        for noise in ("node:internal", "Node.js v25", "ERR_INVALID_TYPESCRIPT_SYNTAX'", "}"):
            self.assertNotIn(noise, cleaned)
        self.assertIn("SyntaxError [ERR_INVALID_TYPESCRIPT_SYNTAX]: Expression expected", cleaned)
        self.assertIn("const x: number = ;", cleaned)

    def test_frames_of_the_program_itself_are_kept(self):
        cleaned = run.clean_node_stderr(self.CRASH)
        self.assertIn("at boom (main.ts:3:31)", cleaned)
        self.assertIn("at Object.<anonymous> (main.ts:4:1)", cleaned)
        self.assertNotIn("node:internal", cleaned)
        self.assertNotIn("Node.js v", cleaned)

    def test_crlf_output_is_handled(self):
        self.assertNotIn("node:internal", run.clean_node_stderr(self.CRASH.replace("\n", "\r\n")))

    def test_syntax_errors_are_told_apart_from_crashes(self):
        self.assertTrue(run._is_node_syntax_error(run.clean_node_stderr(self.SYNTAX)))
        self.assertFalse(run._is_node_syntax_error(run.clean_node_stderr(self.CRASH)))
        # a SyntaxError thrown while the program runs (JSON.parse) has a frame of the program: a crash, not a rejection
        self.assertFalse(run._is_node_syntax_error(run.clean_node_stderr(self.RUNTIME_SYNTAX_ERROR)))
        self.assertFalse(run._is_node_syntax_error(""))

    def test_node_module_urls_lose_the_scratch_directory(self):
        with run.scratch_dir() as wd:
            posix = wd.resolve().as_posix()
            url = ("file:///" if re.match(r"[A-Za-z]:", posix) else "file://") + posix + "/main.ts:3:1"
            self.assertEqual(run.scrub_paths(f"at async {url}", wd), "at async main.ts:3:1")


class ToolchainLookup(unittest.TestCase):
    def test_node_is_found_or_reported(self):
        with self.assertRaises(run.HarnessError) as cm:
            run.find_node("definitely-not-node-xyz")
        self.assertIn("--node", str(cm.exception))
        with mock.patch.object(shutil, "which", return_value=None):
            with self.assertRaises(run.HarnessError) as cm:
                run.find_node()
        self.assertIn("Node.js not found", str(cm.exception))

    @needs_node
    def test_node_version_gate(self):
        lang = run.TypeScriptLang()
        with mock.patch.object(run.TypeScriptLang, "version_text", return_value="v18.19.0"):
            with self.assertRaises(run.HarnessError) as cm:
                lang.preflight()
        self.assertIn("22.6", str(cm.exception))
        with mock.patch.object(run.TypeScriptLang, "version_text", return_value="not node"):
            with self.assertRaises(run.HarnessError):
                lang.preflight()

    def test_language_names_and_aliases(self):
        self.assertEqual([run.canonical_lang(n) for n in ("TS", " rs ", "py", "nyra", "rust")],
                         ["typescript", "rust", "python", "nyra", "rust"])
        self.assertEqual(run.LANG_ORDER, ("nyra", "python", "typescript", "rust"))
        code, _, stderr = _run_main("--langs", "python,cobol", "--dry-run")
        self.assertEqual(code, 2)
        self.assertIn("unknown language 'cobol'", stderr)
        self.assertIn("typescript", stderr)
        self.assertIn("rust", stderr)
        code, _, stderr = _run_main("--langs", "python,py", "--dry-run")
        self.assertEqual(code, 2)
        self.assertIn("distinct", stderr)


class RustToolchainDetection(unittest.TestCase):
    """Finding a rustc that can link: the Windows default (MSVC) fails without Visual Studio, the GNU one works."""

    def test_an_explicit_command_is_the_only_candidate(self):
        self.assertEqual(run.rustc_candidates("rustc +stable-x86_64-pc-windows-gnu"),
                         [["rustc", "+stable-x86_64-pc-windows-gnu"]])
        self.assertEqual(run.rustc_candidates('"C:\\Program Files\\Rust\\rustc.exe" +stable', windows=True),
                         [["C:\\Program Files\\Rust\\rustc.exe", "+stable"]])

    def test_windows_tries_the_gnu_toolchains_first(self):
        toolchains = ["stable-x86_64-pc-windows-msvc", "stable-x86_64-pc-windows-gnu", "nightly-x86_64-pc-windows-gnu"]
        with mock.patch.object(run, "_find_rust_tool", return_value="/x/rustc"), \
                mock.patch.object(run, "_rustup_toolchains", return_value=toolchains):
            self.assertEqual(run.rustc_candidates(windows=True),
                             [["/x/rustc", "+stable-x86_64-pc-windows-gnu"],
                              ["/x/rustc", "+nightly-x86_64-pc-windows-gnu"], ["/x/rustc"]])

    def test_other_platforms_use_the_plain_rustc(self):
        with mock.patch.object(run, "_find_rust_tool", return_value="/x/rustc"), \
                mock.patch.object(run, "_rustup_toolchains", side_effect=AssertionError("not asked")):
            self.assertEqual(run.rustc_candidates(windows=False), [["/x/rustc"]])

    def test_no_rustc_means_no_candidates(self):
        with mock.patch.object(run, "_find_rust_tool", return_value=None):
            self.assertEqual(run.rustc_candidates(), [])
            with self.assertRaises(run.HarnessError) as cm:
                run.RustLang().command()
        self.assertIn("rustc not found", str(cm.exception))
        self.assertIn("rustup.rs", str(cm.exception))

    def test_rustc_is_found_in_the_cargo_bin_directory_when_it_is_not_on_path(self):
        with tempfile.TemporaryDirectory() as home:
            exe = "rustc.exe" if os.name == "nt" else "rustc"
            (Path(home) / "bin").mkdir()
            (Path(home) / "bin" / exe).write_text("")
            with mock.patch.object(shutil, "which", return_value=None), \
                    mock.patch.dict(os.environ, {"CARGO_HOME": home}):
                self.assertEqual(run._find_rust_tool("rustc"), str(Path(home) / "bin" / exe))
                self.assertIsNone(run._find_rust_tool("no-such-tool"))

    def test_the_first_candidate_that_compiles_is_used_and_remembered(self):
        lang = run.RustLang()
        tried = []

        def fake(self_, cmd, code, task):
            tried.append(cmd)
            if cmd == ["good"]:
                return run.EvalResult(True, "pass")
            return run.EvalResult(False, "toolchain_error", stderr="error: linking with `link.exe` failed")

        with mock.patch.object(run, "rustc_candidates", return_value=[["bad"], ["good"]]), \
                mock.patch.object(run.RustLang, "_evaluate_with", fake):
            self.assertEqual(lang.command(), ["good"])
            self.assertEqual(lang.command(), ["good"])
        self.assertEqual(tried, [["bad"], ["good"]])  # probed once

    def test_when_nothing_works_every_attempt_is_explained(self):
        failing = run.EvalResult(False, "toolchain_error", stderr="error: linking with `link.exe` failed")
        with mock.patch.object(run, "rustc_candidates", return_value=[["rustc", "+gnu"], ["rustc"]]), \
                mock.patch.object(run.RustLang, "_evaluate_with", return_value=failing):
            with self.assertRaises(run.HarnessError) as cm:
                run.RustLang().preflight()
        text = str(cm.exception)
        self.assertIn("`rustc +gnu`", text)
        self.assertIn("`rustc`", text)
        self.assertIn("linking", text)
        self.assertIn("GNU toolchain", text)
        self.assertIn("--rustc", text)

    def test_commands_are_shown_without_directories(self):
        self.assertEqual(run._command_label(["C:\\Users\\me\\.cargo\\bin\\rustc.exe", "+stable"]), "rustc +stable")
        self.assertEqual(run._command_label(["/usr/bin/rustc"]), "rustc")

    def test_toolchain_failures_are_told_apart_from_mistakes_in_the_program(self):
        for text in ("error: linking with `link.exe` failed: exit code: 1", "error: linker `cc` not found",
                     "error: internal compiler error: unexpected panic",
                     "error: toolchain 'x' is not installed", "rustup could not choose a version of rustc"):
            self.assertTrue(run._is_rustc_toolchain_failure(text), text)
        for text in ("error[E0425]: cannot find value `x` in this scope", "error: expected one of `;`, found `}`",
                     "error[E0463]: can't find crate for `rand`", "error: aborting due to 1 previous error"):
            self.assertFalse(run._is_rustc_toolchain_failure(text), text)


@needs_node
class EvaluateTypeScript(unittest.TestCase):
    def setUp(self):
        self.lang = run.TypeScriptLang(timeout=10)

    def test_preflight_runs_a_typed_program(self):
        self.assertEqual(self.lang.preflight(), [])
        self.assertGreaterEqual(self.lang.version(), run.NODE_MIN_VERSION)

    def test_types_are_erased_and_output_is_normalized(self):
        code = ("interface P { x: number }\nfunction id<T>(v: T): T { return v; }\n"
                "const p: P = { x: id<number>(3) } as P;\nconsole.log(p.x);\nconsole.log('x   ');\n")
        r = self.lang.evaluate(code, _task("3\nx\n\n"))
        self.assertTrue(r.passed, r)

    def test_enums_namespaces_and_parameter_properties_run(self):
        code = ("enum Color { Red, Green }\nclass Pt { constructor(public x: number, private y: number) {}\n"
                "  sum(): number { return this.x + this.y; } }\nnamespace NS { export const v = 5; }\n"
                "console.log(Color.Green, new Pt(1, 2).sum(), NS.v);\n")
        r = self.lang.evaluate(code, _task("1 3 5\n"))
        self.assertTrue(r.passed, r)

    def test_type_errors_are_not_detected(self):
        # documented limitation: nothing type-checks the program, so a wrong type does not fail it
        r = self.lang.evaluate('const n: number = "text";\nconsole.log(n);\n', _task("text\n"))
        self.assertTrue(r.passed, r)

    def test_import_syntax_and_top_level_await_work(self):
        code = ('import { createHash } from "node:crypto";\nconst v = await Promise.resolve(41 + 1);\n'
                'console.log(createHash("sha256").update("x").digest("hex").length, v);\nexport {};\n')
        r = self.lang.evaluate(code, _task("64 42\n"))
        self.assertTrue(r.passed, r)

    def test_commonjs_require_works(self):
        r = self.lang.evaluate('const util = require("util");\nconsole.log(util.format("%d items", 3));\n',
                               _task("3 items\n"))
        self.assertTrue(r.passed, r)

    def test_wrong_output_feedback_hides_the_expected_output(self):
        r = self.lang.evaluate("console.log(41);\n", _task("42\n"))
        self.assertEqual(r.kind, "wrong_output")
        self.assertIn("41", r.feedback)
        self.assertNotIn("42", r.feedback)
        self.assertIn("line 1", r.feedback)

    def test_a_crash_is_a_runtime_error_with_a_clean_message(self):
        code = 'console.log("before");\nfunction boom(): void { throw new Error("kaboom"); }\nboom();\n'
        r = self.lang.evaluate(code, _task("before\n"))
        self.assertEqual((r.kind, r.exit_code), ("runtime_error", 1))
        for wanted in ("Error: kaboom", "before"):
            self.assertIn(wanted, r.feedback)
        self.assertRegex(r.feedback, r"main\.ts:\d+")  # where it happened, relative to the program (no scratch path)
        for noise in ("node:internal", "Node.js v", "nyra-bench-", "ExperimentalWarning"):
            self.assertNotIn(noise, r.feedback)
            self.assertNotIn(noise, r.stderr)

    def test_a_syntax_error_is_a_compile_error(self):
        r = self.lang.evaluate("const x: number = ;\nconsole.log(x);\n", _task())
        self.assertEqual(r.kind, "compile_error")
        self.assertIn("Node.js rejected your program", r.feedback)
        self.assertIn("SyntaxError", r.feedback)
        self.assertNotIn("nyra-bench-", r.feedback)

    def test_a_syntax_error_raised_while_running_is_a_runtime_error(self):
        r = self.lang.evaluate('console.log(1);\nJSON.parse("x");\n', _task("1\n"))
        self.assertEqual(r.kind, "runtime_error")
        self.assertIn("1", r.feedback)

    def test_nonzero_exit_fails_even_if_the_output_matches(self):
        r = self.lang.evaluate("console.log(1);\nprocess.exit(3);\n", _task("1\n"))
        self.assertEqual((r.passed, r.kind, r.exit_code), (False, "runtime_error", 3))

    def test_timeout_and_output_limit(self):
        lang = run.TypeScriptLang(timeout=5)
        self.assertEqual(lang.evaluate("while (true) {}\n", _task()).kind, "timeout")
        # a generous timeout and megabyte lines: on a slow CI runner the TypeScript start-up alone can take
        # seconds, so with a short timeout the cap would race the clock
        slow_ok = run.TypeScriptLang(timeout=60)
        r = slow_ok.evaluate('const s = "x".repeat(1000000);\nwhile (true) console.log(s);\n', _task())
        self.assertEqual(r.kind, "output_limit")

    def test_secrets_are_not_visible_to_the_program(self):
        with mock.patch.dict(os.environ, {"OPENROUTER_API_KEY": "sk-or-v1-secret"}):
            r = self.lang.evaluate("console.log(process.env.OPENROUTER_API_KEY === undefined);\n", _task("true\n"))
        self.assertTrue(r.passed, r)

    def test_reference_solutions_match_the_expected_outputs(self):
        tasks = run.load_tasks()

        def check(t):
            return t.id, self.lang.evaluate(self.lang.reference_code(t.id), t)

        with cf.ThreadPoolExecutor(max_workers=4) as pool:
            for tid, r in pool.map(check, tasks):
                self.assertTrue(r.passed, (tid, r.kind, r.stderr[-300:]))


class EvaluateRust(NeedsRust):
    def test_preflight_found_a_working_command(self):
        self.assertTrue(self.lang.command_text().startswith("rustc"))
        self.assertTrue(self.lang.version_text().startswith("rustc "), self.lang.version_text())

    def test_pass_and_output_normalization(self):
        code = 'fn main() {\n    println!("1");\n    println!("x   ");\n}\n'
        r = self.lang.evaluate(code, _task("1\nx\n\n"))
        self.assertTrue(r.passed, r)
        self.assertIsNotNone(r.compile_ms)

    def test_edition_2021_is_used(self):
        # arrays iterate by value only from edition 2021 on; plain rustc defaults to 2015 and would reject this
        code = 'fn main() {\n    let v: Vec<i32> = [1, 2, 3].into_iter().collect();\n    println!("{:?}", v);\n}\n'
        r = self.lang.evaluate(code, _task("[1, 2, 3]\n"))
        self.assertTrue(r.passed, r)

    def test_programs_are_built_with_optimizations(self):
        # without overflow checks (release mode) this wraps around to 0; a debug build would panic
        code = 'fn main() {\n    let x: u8 = std::hint::black_box(255);\n    println!("{}", x + 1);\n}\n'
        r = self.lang.evaluate(code, _task("0\n"))
        self.assertTrue(r.passed, r)

    def test_wrong_output_feedback_hides_the_expected_output(self):
        r = self.lang.evaluate('fn main() {\n    println!("41");\n}\n', _task("42\n"))
        self.assertEqual(r.kind, "wrong_output")
        self.assertIn("41", r.feedback)
        self.assertNotIn("42", r.feedback)

    def test_a_panic_is_a_runtime_error_with_the_message(self):
        code = 'fn main() {\n    let v: Vec<i32> = Vec::new();\n    println!("before");\n    println!("{}", v[3]);\n}\n'
        r = self.lang.evaluate(code, _task("before\n"))
        self.assertEqual((r.kind, r.exit_code), ("runtime_error", 101))
        self.assertIn("index out of bounds", r.feedback)
        self.assertIn("before", r.feedback)
        self.assertNotIn("nyra-bench-", r.feedback)

    def test_compile_errors_show_the_errors_and_no_warnings(self):
        code = 'fn main() {\n    let unused = 1;\n    println!("{}", missing);\n}\n'
        r = self.lang.evaluate(code, _task())
        self.assertEqual(r.kind, "compile_error")
        self.assertIn("The Rust compiler (`rustc`) rejected your program", r.feedback)
        self.assertIn("E0425", r.feedback)
        self.assertIn("missing", r.feedback)
        self.assertNotIn("unused variable", r.feedback)
        self.assertNotIn("nyra-bench-", r.feedback)
        self.assertNotIn("\x1b[", r.feedback)  # no colour codes

    def test_warnings_alone_do_not_matter(self):
        r = self.lang.evaluate('fn main() {\n    let unused = 1;\n    println!("42");\n}\n', _task("42\n"))
        self.assertTrue(r.passed, r)

    def test_external_crates_are_a_compile_error_not_a_toolchain_error(self):
        r = self.lang.evaluate('use rand::Rng;\nfn main() {\n    println!("42");\n}\n', _task("42\n"))
        self.assertEqual(r.kind, "compile_error")

    def test_timeout_and_output_limit(self):
        lang = run.RustLang(timeout=5)
        lang.cmd = self.lang.command()
        self.assertEqual(lang.evaluate("fn main() {\n    loop {\n        std::hint::black_box(1);\n    }\n}\n",
                                       _task()).kind, "timeout")
        # long lines: println! flushes every line, so one-character lines need seconds to reach the cap
        flood = 'fn main() {\n    let s = "x".repeat(1000);\n    loop {\n        println!("{}", s);\n    }\n}\n'
        self.assertEqual(lang.evaluate(flood, _task()).kind, "output_limit")

    def test_nonzero_exit_fails_even_if_the_output_matches(self):
        r = self.lang.evaluate('fn main() {\n    println!("1");\n    std::process::exit(3);\n}\n', _task("1\n"))
        self.assertEqual((r.passed, r.kind, r.exit_code), (False, "runtime_error", 3))

    def test_reference_solutions_match_the_expected_outputs(self):
        tasks = run.load_tasks()

        def check(t):
            return t.id, self.lang.evaluate(self.lang.reference_code(t.id), t)

        with cf.ThreadPoolExecutor(max_workers=4) as pool:
            for tid, r in pool.map(check, tasks):
                self.assertTrue(r.passed, (tid, r.kind, r.stderr[-300:]))


class SystemPrompts(unittest.TestCase):
    """The prompts are part of the experiment: they are pinned, so a change is a deliberate decision."""

    RULE = "Reply with exactly one fenced code block that contains the complete program, and no other text."

    def paragraph(self, language):
        return (f"Solve the task you are given with a complete {language} program. The program takes no input, and "
                "only what it prints to standard output is checked, so it must print exactly what the task describes.")

    def test_python_prompt_is_unchanged(self):
        self.assertEqual(run._PYTHON_SYSTEM, "You write programs in Python 3, using only the standard library.\n\n"
                         + self.paragraph("Python") + "\n\n" + self.RULE)

    def test_nyra_prompt_is_unchanged(self):
        text = run._NYRA_SYSTEM.format(spec="SPEC TEXT")
        self.assertEqual(text, "You write programs in Nyra, a new programming language that you have not seen before. "
                         "The complete language specification is below. It is the only documentation you have.\n\n"
                         "<nyra_spec>\nSPEC TEXT\n</nyra_spec>\n\n" + self.paragraph("Nyra") + "\n\n" + self.RULE)

    def test_typescript_and_rust_prompts_are_parallel_to_the_others(self):
        for text, language in ((run._TYPESCRIPT_SYSTEM, "TypeScript"), (run._RUST_SYSTEM, "Rust")):
            self.assertIn(self.paragraph(language), text)
            self.assertTrue(text.endswith(self.RULE))
            self.assertNotIn("spec", text.lower())
        self.assertIn("Node.js", run._TYPESCRIPT_SYSTEM)
        self.assertIn("without checking", run._TYPESCRIPT_SYSTEM)
        self.assertIn("no npm packages", run._TYPESCRIPT_SYSTEM)
        self.assertIn("2021", run._RUST_SYSTEM)
        self.assertIn("rustc", run._RUST_SYSTEM)
        self.assertIn("no external crates", run._RUST_SYSTEM)

    def test_every_language_object_serves_its_own_prompt(self):
        self.assertIs(run.PythonLang().system_prompt, run._PYTHON_SYSTEM)
        if NODE:
            self.assertEqual(run.TypeScriptLang().system_prompt, run._TYPESCRIPT_SYSTEM)
        self.assertEqual(run.RustLang().system_prompt, run._RUST_SYSTEM)

    def test_tasks_prompts_are_shared_not_rewritten_per_language(self):
        # the user message is the task's prompt verbatim, whatever the language
        seen = []

        class Spy(providers.Provider):
            name, default_model = "spy", "spy"

            def complete(self, system, messages, meta):
                seen.append((meta["lang"], messages[0]["content"]))
                return providers.Reply("no code here", providers.Usage(1, 1))

        task = _task("1\n")
        task = run.Task(task.id, task.title, "the one prompt", task.expected_output, "0.1", "math", "", Path("."))
        langs = [run.PythonLang(), run.RustLang()] + ([run.TypeScriptLang()] if NODE else [])
        for lang in langs:
            run.run_one(task, lang, 0, run.RunContext(provider=Spy(), repairs=0, count_tokens=False))
        self.assertEqual({prompt for _, prompt in seen}, {"the one prompt"})
        self.assertEqual(len(seen), len(langs))

# ============================================================================== OpenRouter

API_KEY = "sk-or-v1-0123456789abcdef0123456789abcdef"
NULL = object()  # "content": null in a reply


def ok_body(text="```python\nprint(1)\n```", prompt=100, completion=40, reasoning=None, cost=None, finish="stop",
            model="vendor/model-20260101", provider="Vendor", gen_id="gen-1", content=None):
    """What OpenRouter answers to a chat completion."""
    usage = {"prompt_tokens": prompt, "completion_tokens": completion, "total_tokens": prompt + completion}
    if reasoning is not None:
        usage["completion_tokens_details"] = {"reasoning_tokens": reasoning}
    if cost is not None:
        usage["cost"] = cost
    message = {"role": "assistant", "content": None if content is NULL else (text if content is None else content)}
    body = {"id": gen_id, "object": "chat.completion", "model": model, "usage": usage,
            "choices": [{"index": 0, "finish_reason": finish, "native_finish_reason": finish, "message": message}]}
    if provider:  # some responses name the upstream provider at the top level; the documented place is the metadata
        body["provider"] = provider
    return body


def http_error(status, message="boom", headers=None, **metadata):
    body = {"error": {"code": status, "message": message}}
    if metadata:
        body["error"]["metadata"] = metadata
    return (status, headers or {}, body)


class FakeTransport:
    """The HTTP layer, scripted: each call takes the next item of the script. An item is an exception to raise, a
    (status, headers, body) tuple, or a body (status 200); bodies are dicts (sent as JSON) or bytes. After the
    script it answers with a normal reply."""

    def __init__(self, *script):
        self.script = list(script)
        self.calls = []

    def __call__(self, method, url, headers, body, timeout):
        self.calls.append({"method": method, "url": url, "headers": dict(headers),
                           "body": json.loads(body) if body else None, "timeout": timeout})
        item = self.script.pop(0) if self.script else ok_body()
        if isinstance(item, BaseException):
            raise item
        status, resp_headers, payload = item if isinstance(item, tuple) else (200, {}, item)
        raw = bytes(payload) if isinstance(payload, (bytes, bytearray)) else json.dumps(payload).encode()
        return status, resp_headers, raw


class OpenRouterProviderWithFakeTransport(unittest.TestCase):
    def setUp(self):
        patcher = mock.patch.dict(os.environ, {providers.OPENROUTER_KEY_ENV: API_KEY})
        patcher.start()
        self.addCleanup(patcher.stop)
        self.sleeps = []

    def provider(self, *script, **options):
        self.transport = FakeTransport(*script)
        options.setdefault("jitter", False)
        model = options.pop("model", "vendor/model")
        return providers.OpenRouterProvider(model, transport=self.transport, sleep=self.sleeps.append, **options)

    ASK = ("SYS", [{"role": "user", "content": "task"}], {"task_id": "x", "lang": "python", "attempt": 1})

    def test_request_shape(self):
        p = self.provider(ok_body(), effort="low", max_tokens=1234,
                          extra={"provider": {"order": ["anthropic"], "allow_fallbacks": False},
                                 "reasoning": {"max_tokens": 500}})
        messages = [{"role": "user", "content": "task"}, {"role": "assistant", "content": "reply"},
                    {"role": "user", "content": "fix it"}]
        p.complete("SYS", messages, {})
        call = self.transport.calls[0]
        self.assertEqual((call["method"], call["url"]), ("POST", "https://openrouter.ai/api/v1/chat/completions"))
        self.assertEqual(call["headers"]["Authorization"], f"Bearer {API_KEY}")
        self.assertEqual(call["headers"]["Content-Type"], "application/json")
        self.assertIn("nyra-bench", call["headers"]["User-Agent"])
        self.assertEqual(call["headers"]["X-Title"], "Nyra benchmark")
        self.assertEqual(call["headers"]["X-OpenRouter-Metadata"], "enabled")
        body = call["body"]
        self.assertEqual((body["model"], body["max_tokens"]), ("vendor/model", 1234))
        self.assertEqual(body["messages"], [{"role": "system", "content": "SYS"}] + messages)
        self.assertEqual(body["reasoning"], {"max_tokens": 500, "effort": "low"})
        self.assertEqual(body["provider"], {"order": ["anthropic"], "allow_fallbacks": False})
        self.assertNotIn("temperature", body)
        self.assertNotIn("stream", body)
        self.assertEqual(call["timeout"], 900.0)

    def test_nothing_is_sent_that_was_not_asked_for(self):
        self.provider(ok_body()).complete(*self.ASK)
        self.assertEqual(sorted(self.transport.calls[0]["body"]), ["max_tokens", "messages", "model"])

    def test_reply_and_usage_are_read_from_the_response(self):
        p = self.provider(ok_body(prompt=1500, completion=900, reasoning=800, cost=0.0123))
        reply = p.complete(*self.ASK)
        self.assertEqual(reply.text, "```python\nprint(1)\n```")
        self.assertEqual((reply.usage.input_tokens, reply.usage.output_tokens, reply.usage.reasoning_tokens),
                         (1500, 900, 800))
        self.assertAlmostEqual(reply.usage.cost, 0.0123)
        self.assertFalse(reply.usage.estimated)
        self.assertEqual(reply.usage.to_dict(), {"input_tokens": 1500, "output_tokens": 900, "estimated": False,
                                                 "reasoning_tokens": 800, "cost_usd": 0.0123})
        self.assertEqual((reply.stop_reason, reply.request_id, reply.model, reply.upstream),
                         ("end_turn", "gen-1", "vendor/model-20260101", "Vendor"))
        self.assertGreaterEqual(reply.latency_s, 0)
        self.assertAlmostEqual(p.spent(), 0.0123)

    def test_the_upstream_provider_comes_from_the_router_metadata(self):
        body = ok_body(provider=None)
        body["openrouter_metadata"] = {"strategy": "direct", "endpoints": {"total": 2, "available": [
            {"provider": "Slowpoke", "model": "vendor/model", "selected": False},
            {"provider": "Fireworks", "model": "vendor/model", "selected": True}]}}
        self.assertEqual(self.provider(body).complete(*self.ASK).upstream, "Fireworks")
        self.assertIsNone(self.provider(ok_body(provider=None)).complete(*self.ASK).upstream)
        garbage = ok_body(provider=None)
        garbage["openrouter_metadata"] = {"endpoints": {"available": "none"}}
        self.assertIsNone(self.provider(garbage).complete(*self.ASK).upstream)

    def test_usage_without_thinking_or_cost_keeps_the_dict_small(self):
        reply = self.provider(ok_body()).complete(*self.ASK)
        self.assertEqual(reply.usage.to_dict(), {"input_tokens": 100, "output_tokens": 40, "estimated": False})
        self.assertIsNone(self.provider(ok_body()).spent())

    def test_a_response_without_usage_is_still_a_reply(self):
        body = ok_body()
        del body["usage"]
        reply = self.provider(body).complete(*self.ASK)
        self.assertEqual((reply.usage.input_tokens, reply.usage.output_tokens), (None, None))

    def test_finish_reasons_are_normalized_to_the_names_the_report_uses(self):
        for finish, expected in (("stop", "end_turn"), ("length", "max_tokens"), ("content_filter", "refusal"),
                                 ("tool_calls", "tool_use"), ("weird", "weird")):
            self.assertEqual(self.provider(ok_body(finish=finish)).complete(*self.ASK).stop_reason, expected)

    def test_null_and_multipart_content(self):
        self.assertEqual(self.provider(ok_body(content=NULL, finish="length")).complete(*self.ASK).text, "")
        parts = [{"type": "text", "text": "```py\n"}, {"type": "image"}, {"type": "text", "text": "x\n```"}]
        self.assertEqual(self.provider(ok_body(content=parts)).complete(*self.ASK).text, "```py\nx\n```")

    def test_an_inline_thinking_block_is_not_part_of_the_reply(self):
        # drafts inside <think> contain code blocks that are not the answer
        draft = "<think>\nMaybe:\n```python\nprint(0)\n```\nno.\n</think>\n\n```python\nprint(1)\n```"
        text = self.provider(ok_body(content=draft)).complete(*self.ASK).text
        self.assertEqual(text, "```python\nprint(1)\n```")
        self.assertEqual(run.extract_code(text), "print(1)")
        self.assertEqual(self.provider(ok_body(content="<thinking>x</thinking>```py\ny\n```")).complete(*self.ASK).text,
                         "```py\ny\n```")
        kept = "Look: <think> is a tag.\n```py\nz\n```"  # only a leading block is removed
        self.assertEqual(self.provider(ok_body(content=kept)).complete(*self.ASK).text, kept)

    # ---- the key

    def test_the_key_is_read_from_the_environment_only(self):
        import inspect
        params = inspect.signature(providers.OpenRouterProvider.__init__).parameters
        self.assertFalse([p for p in params if "key" in p.lower()], "the key must not be an argument")
        with mock.patch.dict(os.environ):
            os.environ.pop(providers.OPENROUTER_KEY_ENV)
            p = self.provider(ok_body())
            with self.assertRaises(providers.ProviderError) as cm:
                p.ensure_ready()
        self.assertTrue(cm.exception.fatal and cm.exception.stop_all)
        self.assertIn("OPENROUTER_API_KEY", str(cm.exception))
        self.assertEqual(self.transport.calls, [])

    def test_a_key_with_stray_whitespace_or_quotes_is_handled(self):
        with mock.patch.dict(os.environ, {providers.OPENROUTER_KEY_ENV: f"  {API_KEY}\n"}):
            self.provider(ok_body()).complete(*self.ASK)
            self.assertEqual(self.transport.calls[0]["headers"]["Authorization"], f"Bearer {API_KEY}")
        for bad in ('"sk or v1"', "sk-or-v1-caf" + chr(233)):
            with mock.patch.dict(os.environ, {providers.OPENROUTER_KEY_ENV: bad}):
                with self.assertRaises(providers.ProviderError) as cm:
                    self.provider(ok_body()).ensure_ready()
            self.assertTrue(cm.exception.stop_all)
            self.assertNotIn(bad, str(cm.exception))

    def test_the_key_never_appears_in_descriptions_or_errors(self):
        p = self.provider(ConnectionResetError(f"reset while sending Authorization: Bearer {API_KEY}"), max_retries=0)
        with self.assertRaises(providers.ProviderError) as cm:
            p.complete(*self.ASK)
        self.assertNotIn(API_KEY, str(cm.exception))
        self.assertIn("***", str(cm.exception))
        self.assertNotIn(API_KEY, json.dumps(p.describe()))
        p = self.provider(http_error(500, f"echo {API_KEY}"), max_retries=0)
        with self.assertRaises(providers.ProviderError) as cm:
            p.complete(*self.ASK)
        self.assertNotIn(API_KEY, str(cm.exception))

    def test_a_model_is_required_and_never_guessed(self):
        self.assertEqual(providers.OpenRouterProvider.default_model, "")
        with self.assertRaises(providers.ProviderError) as cm:
            providers.OpenRouterProvider(None).ensure_ready()
        self.assertTrue(cm.exception.fatal)
        self.assertIn("--models", str(cm.exception))

    def test_request_fields_owned_by_the_runner_cannot_be_overridden(self):
        for key in ("model", "messages", "stream"):
            with self.assertRaises(ValueError):
                providers.OpenRouterProvider("a/b", extra={key: 1})

    def test_the_base_url_must_be_https_or_local(self):
        for url in ("http://openrouter.ai/api/v1", "ftp://x/y", "openrouter.ai", ""):
            with self.assertRaises(ValueError, msg=url):
                providers.check_base_url(url)
        for url in ("https://openrouter.ai/api/v1/", "http://127.0.0.1:8080/api/v1", "http://localhost/x",
                    "http://[::1]:9/x"):
            providers.check_base_url(url)
        self.assertEqual(providers.check_base_url("https://example.com/v1/"), "https://example.com/v1")

    # ---- retries

    def test_rate_limits_are_retried_and_retry_after_is_honored(self):
        p = self.provider(http_error(429, "slow down", {"Retry-After": "7"}), http_error(429, "again"), ok_body())
        reply = p.complete(*self.ASK)
        self.assertEqual(reply.text, "```python\nprint(1)\n```")
        self.assertEqual(len(self.transport.calls), 3)
        self.assertEqual(self.sleeps, [7.0, 2.0])  # the server's wish, then the exponential backoff (1, 2, 4, ...)

    def test_backoff_doubles_up_to_the_cap(self):
        p = self.provider(*[http_error(503)] * 6, ok_body(), backoff=1.0, max_backoff=10.0, max_retries=6)
        p.complete(*self.ASK)
        self.assertEqual(self.sleeps, [1.0, 2.0, 4.0, 8.0, 10.0, 10.0])

    def test_jitter_stays_within_a_quarter(self):
        p = self.provider(http_error(503), ok_body(), jitter=True)
        p.complete(*self.ASK)
        self.assertTrue(0.75 <= self.sleeps[0] <= 1.25, self.sleeps)

    def test_server_errors_exhaust_the_retries_then_fail_without_stopping_the_run(self):
        p = self.provider(*[http_error(502, "Provider returned error", provider_name="Vendor", raw="upstream down")] * 9,
                          max_retries=3)
        with self.assertRaises(providers.ProviderError) as cm:
            p.complete(*self.ASK)
        self.assertEqual(len(self.transport.calls), 4)  # the first try and three retries
        self.assertFalse(cm.exception.fatal)
        for wanted in ("HTTP 502", "Provider returned error", "Vendor", "upstream down"):
            self.assertIn(wanted, str(cm.exception))

    def test_connection_problems_and_timeouts_are_retried(self):
        for exc in (ConnectionResetError("reset"), socket.timeout("timed out"), TimeoutError("timed out"),
                    http.client.RemoteDisconnected("closed"), http.client.IncompleteRead(b"x"),
                    OSError("network unreachable")):
            self.sleeps.clear()
            p = self.provider(exc, ok_body())
            self.assertEqual(p.complete(*self.ASK).usage.output_tokens, 40, type(exc))
            self.assertEqual(len(self.sleeps), 1)

    def test_errors_inside_a_200_response_are_retried_too(self):
        embedded_429 = (200, {}, {"error": {"code": 429, "message": "rate limited upstream"}})
        finish_error = ok_body(finish="error", text="partial")
        finish_error["choices"][0]["error"] = {"code": 502, "message": "provider hung up"}
        no_choices = {"id": "gen-2", "choices": []}
        garbage = (200, {}, b"<html>Cloudflare</html>")
        p = self.provider(embedded_429, finish_error, no_choices, garbage, ok_body())
        self.assertEqual(p.complete(*self.ASK).text, "```python\nprint(1)\n```")
        self.assertEqual(len(self.transport.calls), 5)

    def test_a_200_body_error_with_a_bad_request_code_is_not_retried(self):
        p = self.provider((200, {}, {"error": {"code": 400, "message": "context too long"}}))
        with self.assertRaises(providers.ProviderError) as cm:
            p.complete(*self.ASK)
        self.assertTrue(cm.exception.fatal)
        self.assertEqual(len(self.transport.calls), 1)

    def test_whitespace_before_the_json_is_accepted(self):
        p = self.provider((200, {}, b"\n\n   " + json.dumps(ok_body()).encode()))
        self.assertEqual(p.complete(*self.ASK).usage.output_tokens, 40)

    # ---- which failures stop what

    def test_error_statuses_and_what_they_stop(self):
        cases = ((400, True, False), (404, True, False), (422, True, False), (401, True, True), (402, True, True),
                 (403, False, False), (301, False, False))
        for status, fatal, stop_all in cases:
            p = self.provider(http_error(status, "nope"), http_error(status, "nope"), max_retries=3)
            with self.assertRaises(providers.ProviderError) as cm:
                p.complete(*self.ASK)
            self.assertEqual((cm.exception.fatal, cm.exception.stop_all), (fatal, stop_all), status)
            self.assertEqual(len(self.transport.calls), 1, f"{status} must not be retried")
            self.assertIn(f"HTTP {status}", str(cm.exception))

    def test_an_html_error_page_is_summarized(self):
        p = self.provider((524, {}, b"<html>\n<body>A timeout   occurred</body></html>"), max_retries=0)
        with self.assertRaises(providers.ProviderError) as cm:
            p.complete(*self.ASK)
        self.assertIn("HTTP 524", str(cm.exception))
        self.assertNotIn("\n", str(cm.exception))

    def test_a_model_that_keeps_failing_stops_the_run(self):
        p = self.provider(*[http_error(503)] * 20, max_retries=0, max_consecutive_failures=3)
        for _ in range(2):
            with self.assertRaises(providers.ProviderError) as cm:
                p.complete(*self.ASK)
            self.assertFalse(cm.exception.fatal)
        with self.assertRaises(providers.ProviderError) as cm:
            p.complete(*self.ASK)
        self.assertTrue(cm.exception.fatal)
        self.assertIn("3 requests in a row", str(cm.exception))

    def test_a_success_resets_the_failure_count(self):
        p = self.provider(http_error(503), http_error(503), ok_body(), http_error(503), http_error(503),
                          max_retries=0, max_consecutive_failures=3)
        outcomes = []
        for _ in range(5):
            try:
                p.complete(*self.ASK)
                outcomes.append("ok")
            except providers.ProviderError as exc:
                outcomes.append("fatal" if exc.fatal else "error")
        self.assertEqual(outcomes, ["error", "error", "ok", "error", "error"])

    # ---- code tokens

    def count_transport(self, overhead=7):
        """A server whose prompt_tokens are overhead + characters (so the count of a text is its length)."""
        def respond(method, url, headers, body, timeout):
            text = json.loads(body)["messages"][-1]["content"]
            return 200, {}, json.dumps(ok_body(prompt=overhead + len(text), cost=0.00001)).encode()
        return respond

    def test_count_tokens_measures_the_code_alone_by_subtracting_the_overhead(self):
        calls = []
        base = self.count_transport()

        def transport(method, url, headers, body, timeout):
            calls.append(json.loads(body))
            return base(method, url, headers, body, timeout)

        p = providers.OpenRouterProvider("vendor/model", transport=transport, sleep=self.sleeps.append,
                                         extra={"provider": {"order": ["x"]}, "reasoning": {"max_tokens": 5},
                                                "temperature": 0})
        self.assertEqual(p.count_tokens("hello"), 5)  # (7 + 5) - (7 + 1 - 1)
        self.assertEqual(p.count_tokens("print(1)"), 8)
        self.assertEqual(calls[0]["messages"], [{"role": "user", "content": "x"}])  # the baseline, measured once
        self.assertEqual([c["messages"][0]["content"] for c in calls], ["x", "hello", "print(1)"])
        for c in calls:
            self.assertEqual(c["max_tokens"], providers.COUNT_MAX_TOKENS)
            self.assertEqual(c["provider"], {"order": ["x"]})  # same routing as the real requests
            self.assertNotIn("reasoning", c)  # but no thinking settings that could clash with max_tokens=16
            self.assertNotIn("temperature", c)
        self.assertAlmostEqual(p.spent(), 0.00003)  # counting costs money, and it is counted

    def test_counts_are_cached_per_text(self):
        p = self.provider()
        p._post = mock.Mock(side_effect=lambda body: (ok_body(prompt=7 + len(body["messages"][0]["content"])), 0.1))
        self.assertEqual(p.count_tokens("hello"), 5)
        self.assertEqual(p.count_tokens("hello"), 5)
        self.assertEqual(p._post.call_count, 2)  # the baseline and one count

    def test_a_rejected_count_request_switches_counting_off_without_failing_the_run(self):
        p = self.provider(http_error(400, "max_tokens too small"), max_retries=0)
        with contextlib.redirect_stderr(io.StringIO()) as err:
            self.assertIsNone(p.count_tokens("print(1)"))
            self.assertIsNone(p.count_tokens("print(2)"))
        self.assertEqual(len(self.transport.calls), 1)  # no second attempt
        self.assertIn("counting disabled", err.getvalue())
        self.assertIn("vendor/model", err.getvalue())

    def test_transient_count_failures_are_tolerated_a_few_times(self):
        p = self.provider(*[http_error(503)] * 10, max_retries=0)
        with contextlib.redirect_stderr(io.StringIO()):
            results = [p.count_tokens(f"print({i})") for i in range(4)]
        self.assertEqual(results, [None] * 4)
        self.assertFalse(p._count_enabled)  # three misses in a row end it

    def test_nonsense_counts_are_ignored(self):
        p = self.provider(ok_body(prompt=50), ok_body(prompt=20))  # baseline 49; "hello" -> 20 - 49 < 1
        self.assertIsNone(p.count_tokens("hello"))
        self.assertTrue(p._count_enabled)

    def test_counting_can_be_switched_off(self):
        p = self.provider(count_tokens=False)
        self.assertIsNone(p.count_tokens("print(1)"))
        self.assertEqual(self.transport.calls, [])
        self.assertIsNone(self.provider().count_tokens("   "))

    def test_spent_adds_up_over_calls_and_threads(self):
        p = self.provider(*[ok_body(cost=0.01)] * 8)
        with cf.ThreadPoolExecutor(max_workers=4) as pool:
            list(pool.map(lambda _: p.complete(*self.ASK), range(8)))
        self.assertAlmostEqual(p.spent(), 0.08)

    def test_describe_has_what_a_result_file_needs_and_no_secret(self):
        d = self.provider(effort="high", extra={"x": 1}).describe()
        self.assertEqual(d, {"name": "openrouter", "model": "vendor/model", "base_url": "https://openrouter.ai/api/v1",
                             "max_tokens": 16000, "effort": "high", "extra": {"x": 1}})


class _StubServer(http.server.ThreadingHTTPServer):
    """A little web server on this machine, so the real urllib code runs without any outside network."""

    def __init__(self, handler):
        super().__init__(("127.0.0.1", 0), handler)
        self.requests = []
        self.thread = threading.Thread(target=self.serve_forever, daemon=True)

    def __enter__(self):
        self.thread.start()
        return self

    def __exit__(self, *exc):
        self.shutdown()
        self.server_close()

    @property
    def url(self):
        return f"http://127.0.0.1:{self.server_port}"


class _TransportHandler(http.server.BaseHTTPRequestHandler):
    def _answer(self, status, body, extra_headers=None, delay=0.0):
        if delay:
            time.sleep(delay)
        data = body if isinstance(body, bytes) else json.dumps(body).encode()
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(data)))
        for k, v in (extra_headers or {}).items():
            self.send_header(k, v)
        self.end_headers()
        try:
            self.wfile.write(data)
        except OSError:
            pass

    def _handle(self):
        length = int(self.headers.get("content-length") or 0)
        raw = self.rfile.read(length) if length else b""
        self.server.requests.append((self.command, self.path, {k.lower(): v for k, v in self.headers.items()}, raw))
        if self.path == "/echo":
            self._answer(200, {"method": self.command, "body": json.loads(raw) if raw else None})
        elif self.path == "/padded":
            self._answer(200, b"\n\n   " + json.dumps({"ok": True}).encode())
        elif self.path == "/missing":
            self._answer(404, {"error": {"code": 404, "message": "no such model"}})
        elif self.path == "/redirect":
            self._answer(302, b"", {"Location": self.server.url + "/echo"})
        elif self.path == "/slow":
            self._answer(200, {"late": True}, delay=1.5)
        else:
            self._answer(500, {"error": {"message": "unexpected path"}})

    do_GET = do_POST = _handle

    def log_message(self, *args):
        pass


class UrllibTransportOnThisMachine(unittest.TestCase):
    def test_a_json_round_trip_with_headers(self):
        with _StubServer(_TransportHandler) as server:
            status, headers, raw = providers.urllib_transport(
                "POST", server.url + "/echo", {"Authorization": "Bearer abc", "Content-Type": "application/json"},
                json.dumps({"a": 1}).encode(), 5)
        self.assertEqual(status, 200)
        self.assertEqual(json.loads(raw), {"method": "POST", "body": {"a": 1}})
        method, path, sent, _ = server.requests[0]
        self.assertEqual((method, sent["authorization"], sent["content-type"]), ("POST", "Bearer abc", "application/json"))
        self.assertTrue(any(k.lower() == "content-length" for k in headers))

    def test_error_statuses_are_returned_not_raised(self):
        with _StubServer(_TransportHandler) as server:
            status, _, raw = providers.urllib_transport("GET", server.url + "/missing", {}, None, 5)
        self.assertEqual(status, 404)
        self.assertEqual(json.loads(raw)["error"]["message"], "no such model")

    def test_leading_whitespace_before_the_json_is_parsed(self):
        with _StubServer(_TransportHandler) as server:
            _, _, raw = providers.urllib_transport("GET", server.url + "/padded", {}, None, 5)
        self.assertEqual(providers.loads_json(raw), {"ok": True})
        self.assertIsNone(providers.loads_json(b"<html>"))
        self.assertIsNone(providers.loads_json(b""))

    def test_redirects_are_not_followed_so_the_key_cannot_leak_elsewhere(self):
        with _StubServer(_TransportHandler) as server:
            status, headers, _ = providers.urllib_transport("POST", server.url + "/redirect",
                                                            {"Authorization": "Bearer abc"}, b"{}", 5)
        self.assertEqual(status, 302)
        self.assertEqual([r[1] for r in server.requests], ["/redirect"])

    def test_a_slow_server_times_out_with_an_oserror(self):
        with _StubServer(_TransportHandler) as server:
            with self.assertRaises(OSError):
                providers.urllib_transport("GET", server.url + "/slow", {}, None, 0.3)

    def test_a_closed_port_is_an_oserror(self):
        with _StubServer(_TransportHandler) as server:
            url = server.url
        with self.assertRaises(OSError):
            providers.urllib_transport("GET", url + "/echo", {}, None, 2)


# ======================================================================= model list


LISTING = [
    {"id": "anthropic/claude-opus-5.5", "name": "Anthropic: Claude Opus 5.5", "created": 300, "context_length": 1000000,
     "pricing": {"prompt": "0.000004", "completion": "0.00002"}, "reasoning": {"mandatory": True}},
    {"id": "anthropic/claude-haiku-4.5", "name": "Anthropic: Claude Haiku 4.5", "created": 100, "context_length": 200000,
     "pricing": {"prompt": "0.000001", "completion": "0.000005"}, "reasoning": {"mandatory": False}},
    {"id": "openai/gpt-6-sol", "name": "OpenAI: GPT-6 Sol", "created": 200, "context_length": 1050000,
     "pricing": {"prompt": "0.000002", "completion": "0.00001"}, "reasoning": {"default_enabled": True}},
    {"id": "meta-llama/llama-4-maverick", "name": "Meta: Llama 4 Maverick", "created": 50, "context_length": 1048576,
     "pricing": {"prompt": "weird", "completion": "0.0000006525"}},
]


class ModelList(unittest.TestCase):
    def test_the_public_list_is_fetched_without_credentials(self):
        seen = []

        def transport(method, url, headers, body, timeout):
            seen.append((method, url, dict(headers), body))
            return 200, {}, json.dumps({"data": LISTING + [{"name": "no id"}, "junk"]}).encode()

        with mock.patch.dict(os.environ, {providers.OPENROUTER_KEY_ENV: API_KEY}):
            models = modelsmod.fetch_models(transport=transport)
        self.assertEqual([m["id"] for m in models], [m["id"] for m in LISTING])
        method, url, headers, body = seen[0]
        self.assertEqual((method, url, body), ("GET", "https://openrouter.ai/api/v1/models", None))
        self.assertFalse([h for h in headers if h.lower() == "authorization"], "the list is public: no key is sent")

    def test_fetch_errors_are_modelserrors(self):
        for transport in (lambda *a: (503, {}, b"down"), lambda *a: (200, {}, b"<html>"),
                          lambda *a: (200, {}, b'{"data": 3}'), mock.Mock(side_effect=OSError("offline")),
                          mock.Mock(side_effect=http.client.IncompleteRead(b"x"))):
            with self.assertRaises(modelsmod.ModelsError):
                modelsmod.fetch_models(transport=transport)
        with self.assertRaises(ValueError):
            modelsmod.fetch_models("http://openrouter.ai/api/v1")

    def test_the_shipped_default_list_is_well_formed(self):
        data = modelsmod.load_default_models()
        ids = modelsmod.default_model_ids()
        self.assertTrue(3 <= len(ids) <= 12, ids)
        self.assertEqual(len(set(ids)), len(ids))
        self.assertRegex(data["verified"], r"^\d{4}-\d{2}-\d{2}$")
        for mid in ids:
            self.assertRegex(mid, r"^[a-z0-9][a-z0-9._-]*/[A-Za-z0-9._-]+$", mid)  # vendor/model, nothing else
            self.assertFalse(mid.startswith("~"), "moving -latest aliases are not benchmark material")
        self.assertTrue(any(m.startswith("anthropic/") for m in ids))
        self.assertTrue(any(m.startswith("google/") for m in ids))
        self.assertTrue(any(m.startswith("openai/") for m in ids))

    def test_the_default_model_file_says_where_the_ids_come_from_and_how_to_check_them(self):
        data = modelsmod.load_default_models()
        self.assertIn("python bench/models.py", data["comment"])
        self.assertIn("openrouter.ai/api/v1/models", data["source"])

    def test_openrouter_needs_no_installed_package(self):
        text = (BENCH_DIR / "requirements.txt").read_text(encoding="utf-8")
        packages = [ln.split(">")[0].split("=")[0].strip() for ln in text.splitlines()
                    if ln.strip() and not ln.startswith("#")]
        self.assertEqual(packages, ["anthropic"])  # only the Anthropic provider needs an SDK; the rest is stdlib

    def test_a_broken_default_list_is_reported(self):
        with tempfile.TemporaryDirectory() as tmp:
            for text in ("not json", "{}", '{"models": []}', '{"models": [{"id": "a b"}]}',
                         '{"models": ["x/y", "x/y"]}', '{"models": [{"nid": 1}]}'):
                path = Path(tmp) / "m.json"
                path.write_text(text)
                with self.assertRaises(modelsmod.ModelsError, msg=text):
                    modelsmod.load_default_models(path)
            path.write_text('{"models": ["a/b", {"id": "c/d"}]}')
            self.assertEqual(modelsmod.default_model_ids(path), ["a/b", "c/d"])

    def test_missing_ids_come_with_suggestions(self):
        bad = modelsmod.missing_ids(LISTING, ["openai/gpt-6-sol", "anthropic/claude-opus-5.6", "nonsense"])
        self.assertEqual([b[0] for b in bad], ["anthropic/claude-opus-5.6", "nonsense"])
        self.assertIn("anthropic/claude-opus-5.5", bad[0][1])
        self.assertEqual(bad[1][1], [])

    def test_variant_suffixes_fall_back_to_the_model_itself(self):
        # :online, :nitro, :floor ... are routing variants of a listed model and are not listed themselves
        self.assertEqual(modelsmod.missing_ids(LISTING, ["openai/gpt-6-sol:online", "openai/gpt-6-sol:nitro"]), [])
        bad = modelsmod.missing_ids(LISTING, ["openai/gpt-7:online"])
        self.assertEqual([b[0] for b in bad], ["openai/gpt-7:online"])
        self.assertEqual(modelsmod.lookup({"a/b": 1}, "a/b:x"), 1)
        self.assertEqual(modelsmod.lookup({"a/b": 1, "a/b:x": 2}, "a/b:x"), 2)  # an exact id wins
        self.assertIsNone(modelsmod.lookup({"a/b": 1}, "c/d"))

    def test_a_price_of_minus_one_is_no_price(self):
        # routers such as openrouter/auto have no fixed price and say -1
        self.assertIsNone(modelsmod.price_per_token({"pricing": {"prompt": "-1", "completion": "-1"}}))
        self.assertEqual(modelsmod.price_per_token({"pricing": {"prompt": "0", "completion": "0"}}), (0.0, 0.0))

    def test_describe_survives_missing_fields(self):
        text = modelsmod.describe({"id": "a/b", "context_length": None, "pricing": {}})
        self.assertIn("a/b", text)
        self.assertIn("0 ctx", text)
        self.assertIn("price n/a", text)

    def test_search_matches_every_word_and_lists_newest_first(self):
        self.assertEqual([m["id"] for m in modelsmod.search(LISTING, ["claude"])],
                         ["anthropic/claude-opus-5.5", "anthropic/claude-haiku-4.5"])
        self.assertEqual([m["id"] for m in modelsmod.search(LISTING, ["CLAUDE", "haiku"])], ["anthropic/claude-haiku-4.5"])
        self.assertEqual(modelsmod.search(LISTING, ["nothing"]), [])

    def test_prices_and_descriptions(self):
        self.assertEqual(modelsmod.price_per_token(LISTING[0]), (0.000004, 0.00002))
        self.assertIsNone(modelsmod.price_per_token(LISTING[3]))
        self.assertIsNone(modelsmod.price_per_token({"id": "x"}))
        self.assertIn("$4.00 in / $20.00 out per M tokens", modelsmod.describe(LISTING[0]))
        self.assertIn("thinks always", modelsmod.describe(LISTING[0]))
        self.assertIn("thinking optional", modelsmod.describe(LISTING[1]))
        self.assertIn("thinks by default", modelsmod.describe(LISTING[2]))
        self.assertIn("price n/a", modelsmod.describe(LISTING[3]))

    def run_cli(self, *argv):
        out, err = io.StringIO(), io.StringIO()
        with mock.patch.object(modelsmod, "fetch_models", return_value=LISTING), \
                contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = modelsmod.main(list(argv))
        return code, out.getvalue(), err.getvalue()

    def test_cli_check(self):
        code, out, _ = self.run_cli("--check", "--ids", "openai/gpt-6-sol,anthropic/claude-opus-5.5")
        self.assertEqual(code, 0)
        self.assertIn("2 of 2 id(s) exist", out)
        code, out, _ = self.run_cli("--check", "--ids", "openai/gpt-6-sol,openai/gpt-7")
        self.assertEqual(code, 1)
        self.assertIn("MISSING openai/gpt-7", out)

    def test_cli_search_and_json(self):
        code, out, _ = self.run_cli("claude", "opus")
        self.assertEqual(code, 0)
        self.assertIn("anthropic/claude-opus-5.5", out)
        self.assertNotIn("haiku", out)
        code, out, _ = self.run_cli("--json", "gpt")
        self.assertEqual(json.loads(out)[0]["id"], "openai/gpt-6-sol")
        self.assertEqual(self.run_cli("zzz")[0], 1)

    def test_cli_default_list_names_the_models_that_exist(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "m.json"
            path.write_text('{"verified": "2026-01-01", "models": ["openai/gpt-6-sol", "gone/model"]}')
            code, out, _ = self.run_cli("--file", str(path))
        self.assertEqual(code, 0)
        self.assertIn("verified 2026-01-01", out)
        self.assertIn("openai/gpt-6-sol", out)
        self.assertNotIn("gone/model", out)

    def test_cli_reports_an_unreachable_list(self):
        out, err = io.StringIO(), io.StringIO()
        with mock.patch.object(modelsmod, "fetch_models", side_effect=modelsmod.ModelsError("cannot reach")), \
                contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            self.assertEqual(modelsmod.main(["claude"]), 2)
        self.assertIn("cannot reach", err.getvalue())


class _FakeOpenRouter(http.server.BaseHTTPRequestHandler):
    """Enough of OpenRouter for the runner: GET /api/v1/models and POST /api/v1/chat/completions. It answers with the
    reference solution of the task in the language of the system prompt. A model whose id contains `weak` answers
    its first attempt without a code block; `broken` answers with a server error."""

    def _answer(self, status, body):
        data = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        self.server.requests.append(("GET", self.path, {k.lower(): v for k, v in self.headers.items()}, None))
        if self.path.endswith("/models"):
            self._answer(200, {"data": [{"id": m, "name": m, "created": 1, "context_length": 1000,
                                         "pricing": {"prompt": "0.000001", "completion": "0.000002"},
                                         "top_provider": {"max_completion_tokens": 8000 if "small" in m else None}}
                                        for m in self.server.models]})
        else:
            self._answer(404, {"error": {"code": 404, "message": "no route"}})

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get("content-length", 0))))
        self.server.requests.append(("POST", self.path, {k.lower(): v for k, v in self.headers.items()}, body))
        if not self.path.endswith("/chat/completions"):
            return self._answer(404, {"error": {"code": 404, "message": "no route"}})
        model = body["model"]
        if model not in self.server.models:
            return self._answer(404, {"error": {"code": 404, "message": f"No endpoints found for {model}"}})
        if "broken" in model:
            return self._answer(500, {"error": {"code": 500, "message": "upstream exploded"}})
        if body.get("max_tokens") == providers.COUNT_MAX_TOKENS:  # a token-count request: code alone, one message
            text = body["messages"][-1]["content"]
            return self._answer(200, ok_body(prompt=10 + len(text) // 3, completion=1, cost=0.000001, model=model))
        system, user_messages = body["messages"][0]["content"], body["messages"][1:]
        task_prompt = user_messages[0]["content"]
        lang = ("nyra" if "<nyra_spec>" in system else "python" if "Python 3" in system
                else "typescript" if "TypeScript" in system else "rust")
        task = next(t for t in self.server.tasks if t.prompt == task_prompt)
        if "weak" in model and len(user_messages) == 1:
            text = "I would solve this with a loop."
        else:
            text = f"```{lang}\n{reference_for_tests(lang, task.id).rstrip()}\n```"
        self._answer(200, ok_body(text=text, prompt=len(system) // 4 + 50, completion=60 if "weak" not in model else 30,
                                  reasoning=10, cost=None if "nocost" in model else 0.0005, model=model + "-20260101",
                                  provider="Fake Inc"))

    def log_message(self, *args):
        pass


def _fake_openrouter(models, tasks):
    server = _StubServer(_FakeOpenRouter)
    server.models, server.tasks = list(models), tasks
    return server


def reference_for_tests(lang: str, task_id: str) -> str:
    """What the fake OpenRouter answers with: the reference solution of the task."""
    ext = {"python": ".py", "typescript": ".ts", "rust": ".rs", "nyra": ".nyra"}[lang]
    return (run.SOLUTIONS_DIR / lang / f"{task_id}{ext}").read_text(encoding="utf-8")


class OpenRouterRunEndToEnd(unittest.TestCase):
    """run.py against a fake OpenRouter on this machine: real urllib, real files, no outside network."""

    TASKS = "fizzbuzz,gcd_pairs"

    def setUp(self):
        patcher = mock.patch.dict(os.environ, {providers.OPENROUTER_KEY_ENV: API_KEY})
        patcher.start()
        self.addCleanup(patcher.stop)
        self.tasks = run.load_tasks()

    def run_main(self, server, *argv):
        base = server.url + "/api/v1"
        return _run_main("--provider", "openrouter", "--base-url", base, "--langs", "python", "--tasks", self.TASKS,
                         "-q", "--jobs", "2", *argv)

    def test_two_models_one_run_and_a_comparison(self):
        with _fake_openrouter(["good/model", "weak/model"], self.tasks) as server, tempfile.TemporaryDirectory() as out:
            code, stdout, stderr = self.run_main(server, "--models", "good/model,weak/model", "--out", out)
            files = sorted(p.name for p in Path(out).iterdir())
            loaded = [json.loads(p.read_text(encoding="utf-8")) for p in Path(out).glob("*.json")]
            results = {r["run"]["provider"]["model"]: r for r in loaded if "records" in r}
            index = next(r for r in loaded if "records" not in r)
            compare = next(Path(out).glob("*compare.md")).read_text(encoding="utf-8")
            everything = "".join(p.read_text(encoding="utf-8") for p in Path(out).iterdir())
        self.assertEqual(code, 0, stderr)
        self.assertEqual(len(files), 7)  # two results (json + md each), the comparison (md + json) and latest.md
        self.assertIn("latest.md", files)
        good, weak = results["good/model"], results["weak/model"]
        self.assertEqual(good["summary"]["langs"]["python"]["pass_at_1"], 2)
        self.assertEqual(weak["summary"]["langs"]["python"]["pass_at_1"], 0)
        self.assertEqual(weak["summary"]["langs"]["python"]["pass_within_repairs"], 2)
        self.assertEqual(weak["records"][0]["attempts"][0]["result"]["kind"], "no_code")
        # what the API reported is kept
        a = good["records"][0]["attempts"][0]
        self.assertEqual((a["usage"]["output_tokens"], a["usage"]["reasoning_tokens"]), (60, 10))
        self.assertEqual(a["served_model"], "good/model-20260101")
        self.assertEqual(a["served_by"], "Fake Inc")
        self.assertIsNotNone(a["code_tokens"])  # counted with a token-count request
        self.assertIsNone(weak["records"][0]["attempts"][1]["code_tokens"])  # repairs are not counted
        self.assertEqual(good["run"]["served_models"], ["good/model-20260101"])
        self.assertGreater(good["run"]["spent_usd"], 0.001)
        self.assertEqual(good["run"]["provider"]["base_url"], server.url + "/api/v1")
        # the comparison names both models and the index lists their files
        self.assertIn("good/model", compare)
        self.assertIn("weak/model", compare)
        self.assertIn("First-try success", compare)
        self.assertEqual([m["model"] for m in index["models"]], ["good/model", "weak/model"])
        # the key travelled in the Authorization header of the chat requests only, never into a file or the output
        chat = [r for r in server.requests if r[0] == "POST"]
        self.assertTrue(chat)
        self.assertTrue(all(r[2]["authorization"] == f"Bearer {API_KEY}" for r in chat))
        listing = [r for r in server.requests if r[0] == "GET"]
        self.assertTrue(listing and all("authorization" not in r[2] for r in listing))
        for text in (everything, stdout, stderr):
            self.assertNotIn(API_KEY, text)
        self.assertIn("spent $", stdout)

    def test_unknown_model_ids_are_refused_before_anything_is_spent(self):
        with _fake_openrouter(["good/model"], self.tasks) as server, tempfile.TemporaryDirectory() as out:
            code, stdout, stderr = self.run_main(server, "--models", "good/model,good/modle", "--out", out)
            self.assertEqual(list(Path(out).iterdir()), [])
        self.assertEqual(code, 2)
        self.assertIn("good/modle", stderr)
        self.assertIn("similar: good/model", stderr)
        self.assertEqual([r for r in server.requests if r[0] == "POST"], [])

    def test_a_variant_of_a_listed_model_passes_the_id_check(self):
        with _fake_openrouter(["small/model"], self.tasks) as server, tempfile.TemporaryDirectory() as out:
            code, stdout, stderr = self.run_main(server, "--models", "small/model:online", "--out", out, "--dry-run")
        self.assertEqual(code, 0, stderr)
        self.assertRegex(stdout, r"small/model:online: about \$\d")
        self.assertIn("small/model:online allows", stderr)  # the limits of the model itself apply to its variants

    def test_a_max_tokens_above_what_a_model_allows_is_warned_about_up_front(self):
        with _fake_openrouter(["small/model", "good/model"], self.tasks) as server, tempfile.TemporaryDirectory() as out:
            code, _, stderr = self.run_main(server, "--models", "small/model,good/model", "--out", out, "--dry-run")
            self.assertEqual(code, 0)
            self.assertIn("--max-tokens 16000 is above the 8000 completion tokens small/model allows", stderr)
            self.assertNotIn("good/model allows", stderr)
            _, _, stderr = self.run_main(server, "--models", "small/model", "--out", out, "--dry-run", "--max-tokens", "8000")
            self.assertNotIn("allows", stderr)

    def test_the_model_check_can_be_skipped_and_an_unreachable_list_is_only_a_warning(self):
        with _fake_openrouter(["good/model"], self.tasks) as server, tempfile.TemporaryDirectory() as out:
            code, _, stderr = self.run_main(server, "--models", "unlisted/model", "--no-model-check", "--out", out)
            self.assertEqual(code, 2)  # the server answers 404 to the first request: that stops the model
            self.assertIn("No endpoints found for unlisted/model", stderr)
            self.assertEqual([r for r in server.requests if r[0] == "GET"], [])
            self.assertEqual(list(Path(out).iterdir()), [])  # nothing finished, so no files
        # unreachable: the list cannot be fetched, the run goes on and the first request decides
        with tempfile.TemporaryDirectory() as out:
            code, _, stderr = _run_main("--provider", "openrouter", "--base-url", "http://127.0.0.1:9/api/v1",
                                        "--models", "a/b", "--langs", "python", "--tasks", "fizzbuzz", "--out", out,
                                        "-q", "--dry-run")
        self.assertEqual(code, 0)
        self.assertIn("could not check the model ids", stderr)

    def test_dry_run_estimates_the_cost_and_calls_nothing(self):
        with _fake_openrouter(["good/model", "weak/model"], self.tasks) as server, tempfile.TemporaryDirectory() as out:
            code, stdout, _ = self.run_main(server, "--models", "good/model,weak/model", "--out", out, "--dry-run")
            self.assertEqual(list(Path(out).iterdir()), [])
        self.assertEqual(code, 0)
        self.assertIn("at most 8 model calls per model, 16 in total", stdout)
        self.assertIn("estimated cost", stdout)
        self.assertRegex(stdout, r"good/model: about \$\d")
        self.assertRegex(stdout, r"total: about \$\d")
        self.assertIn("--budget", stdout)
        self.assertEqual([r for r in server.requests if r[0] == "POST"], [])

    def test_a_bad_key_stops_the_whole_run_and_a_missing_key_stops_it_before_it_starts(self):
        with _fake_openrouter(["a/model", "b/model"], self.tasks) as server, tempfile.TemporaryDirectory() as out:
            with mock.patch.object(providers.OpenRouterProvider, "_post", side_effect=providers.ProviderError(
                    "HTTP 401: No auth credentials found", stop_all=True)):
                code, _, stderr = self.run_main(server, "--models", "a/model,b/model", "--out", out)
            self.assertEqual(code, 2)
            self.assertIn("HTTP 401", stderr)
            self.assertIn("remaining 1 model(s) were not run", stderr)
            self.assertEqual(list(Path(out).iterdir()), [])
            with mock.patch.dict(os.environ):
                os.environ.pop(providers.OPENROUTER_KEY_ENV)
                code, _, stderr = self.run_main(server, "--models", "a/model", "--out", out)
            self.assertEqual(code, 2)
            self.assertIn("OPENROUTER_API_KEY", stderr)
            self.assertEqual([r for r in server.requests if r[0] == "POST"], [])

    def test_a_model_that_fails_does_not_stop_the_others(self):
        with _fake_openrouter(["broken/model", "good/model"], self.tasks) as server, tempfile.TemporaryDirectory() as out:
            with mock.patch.object(providers.OpenRouterProvider, "__init__", _fast_init(max_retries=0)):
                code, _, stderr = self.run_main(server, "--models", "broken/model,good/model", "--out", out)
            names = sorted(p.name for p in Path(out).glob("*.json") if "compare" not in p.name)
        self.assertEqual(code, 2, stderr)  # something went wrong, and it is said so ...
        self.assertIn("no run finished for broken/model", stderr)
        self.assertEqual(len(names), 1)  # ... but the other model was run and saved
        self.assertTrue(names[0].endswith("-openrouter-good-model.json"), names)

    def test_the_budget_stops_after_the_model_that_used_it_up(self):
        with _fake_openrouter(["good/model", "other/model"], self.tasks) as server, tempfile.TemporaryDirectory() as out:
            code, stdout, stderr = self.run_main(server, "--models", "good/model,other/model", "--out", out,
                                                 "--budget", "0.0007", "--jobs", "1", "--no-count-tokens")
            files = sorted(p.name for p in Path(out).glob("*.json") if "compare" not in p.name)
            result = json.loads(Path(out, files[0]).read_text(encoding="utf-8"))
        self.assertEqual(code, 2)
        self.assertIn("budget of $0.0007 was reached", stderr)
        self.assertIn("the remaining 1 model(s) were not run", stderr)
        self.assertEqual(len(files), 1, files)  # the second model was never started
        self.assertTrue(result["run"]["complete"])  # the first model's runs had all finished
        self.assertEqual(result["run"]["budget_usd"], 0.0007)
        self.assertIn("spent $0.0010 of the $0.0007 budget", stdout)

    def test_the_budget_can_end_a_model_in_the_middle(self):
        with _fake_openrouter(["good/model"], self.tasks) as server, tempfile.TemporaryDirectory() as out:
            code, stdout, stderr = self.run_main(server, "--models", "good/model", "--out", out, "--budget", "0.0003",
                                                 "--jobs", "1", "--no-count-tokens")
            result = json.loads(next(Path(out).glob("*good-model.json")).read_text(encoding="utf-8"))
        self.assertEqual(code, 2)
        self.assertFalse(result["run"]["complete"])
        self.assertEqual([r["status"] for r in result["records"]], ["pass", "aborted"])
        self.assertIn("INCOMPLETE RUN", stdout)
        self.assertIn("were not started or finished", stderr)

    def test_a_budget_cannot_work_without_reported_costs_and_that_is_said(self):
        with _fake_openrouter(["nocost/model"], self.tasks) as server, tempfile.TemporaryDirectory() as out:
            code, _, stderr = self.run_main(server, "--models", "nocost/model", "--out", out, "--budget", "0.0001",
                                            "--no-count-tokens")
        self.assertEqual(code, 0, stderr)  # nothing could stop it, so it ran to the end
        self.assertIn("--budget could not work for nocost/model: the API reported no costs", stderr)

    def test_budget_needs_openrouter(self):
        code, _, stderr = _run_main("--provider", "mock", "--budget", "1", "--dry-run")
        self.assertEqual(code, 2)
        self.assertIn("--budget", stderr)
        code, _, stderr = _run_main("--provider", "openrouter", "--budget", "-1", "--models", "a/b", "--dry-run")
        self.assertEqual(code, 2)


def _fast_init(**fixed):
    """OpenRouterProvider.__init__ with some options forced (no waiting between retries in a test)."""
    original = providers.OpenRouterProvider.__init__

    def init(self, model=None, **options):
        options.update(fixed)
        options.setdefault("sleep", lambda s: None)
        original(self, model, **options)

    return init

# =========================================================== several models, several languages


def fake_results(model, recs, langs, categories=None, mock=False, **run_fields):
    """A results dict as run.py writes it (without the prompts), built from records."""
    categories = categories or {}
    run_meta = {
        "date": "2026-10-07", "started_at": "2026-10-07T10:00:00+00:00", "finished_at": "2026-10-07T10:30:00+00:00",
        "provider": {"name": "mock" if mock else "openrouter", "model": model}, "langs": langs, "repairs": 3,
        "samples": 1, "timeout_s": 10, "backend": "native", "mock": mock, "complete": True,
        "tokens_are_estimates": mock, "nyra": {"path": "target/release/nyra", "version": "nyra 0.2.0"},
        "spec": {"path": "docs/SPEC.md", "version": "0.2", "sha256": "5" * 64}, "node": {"version": "v25.2.1", "flags": ""},
        "rust": {"command": "rustc", "version": "rustc 1.99.0", "flags": ""}, "python": "3.14.2",
        "max_version": "0.2", "tasks_sha256": "t" * 64, "served_models": [] if mock else [model + "-20260101"],
        "served_by": [], "spent_usd": None, "repo": {"commit": "abc1234", "dirty": False}, "warnings": [],
        "task_ids": sorted({r["task_id"] for r in recs}), "excluded_tasks": [],
        "tasks": {t: {"category": categories.get(t, "math"), "prompt": "PROMPT-" + t}
                  for t in {r["task_id"] for r in recs}},
        "system_prompts": {lang: "SYSTEM-" + lang for lang in langs},
    }
    run_meta.update(run_fields)
    return {"schema": 2, "run": run_meta, "records": recs, "summary": report.summarize(recs, langs, categories)}


def four_language_records(first_try):
    """first_try: {(task, lang): bool}. A run that fails its first try passes on the second (wrong output)."""
    rec = ReportSummary.rec
    out = []
    for (task, lang), ok in first_try.items():
        out.append(rec(task, lang, ok, True, kind="wrong_output", attempts=1 if ok else 2,
                       out={"nyra": 80, "python": 60, "typescript": 90, "rust": 120}[lang],
                       code_tokens={"nyra": 50, "python": 40, "typescript": 60, "rust": 100}[lang]))
    return out


LANGS4 = ["nyra", "python", "typescript", "rust"]


class ReportManyLanguages(unittest.TestCase):
    TASKS = ("a", "b", "c", "d")

    def records(self):
        # a: everybody first try. b: only nyra misses. c: only rust hits. d: nobody
        table = {"a": (1, 1, 1, 1), "b": (0, 1, 1, 1), "c": (0, 0, 0, 1), "d": (0, 0, 0, 0)}
        return four_language_records({(t, lang): bool(table[t][i]) for t in self.TASKS for i, lang in enumerate(LANGS4)})

    def test_every_language_is_compared_with_the_first_one(self):
        s = report.summarize(self.records(), LANGS4)
        self.assertEqual(s["paired"]["baseline"], "nyra")
        self.assertEqual(sorted(s["paired"]["pairs"]), ["python", "rust", "typescript"])
        self.assertEqual(s["paired"]["n"], 1)  # task a is the only run all four got right
        py = s["paired"]["pairs"]["python"]
        self.assertEqual((py["n"], py["first_try"]["only_a"], py["first_try"]["only_b"], py["first_try"]["neither"]),
                         (1, 0, 1, 2))
        self.assertEqual(py["langs"]["nyra"]["avg_code_tokens"], 50)
        self.assertEqual(py["langs"]["python"]["avg_code_tokens"], 40)
        rust = s["paired"]["pairs"]["rust"]
        self.assertEqual((rust["n"], rust["first_try"]["only_b"]), (1, 2))
        self.assertAlmostEqual(rust["first_try"]["sign_test_p"], report.mcnemar_exact(0, 2))
        for lang in LANGS4:
            self.assertEqual(s["paired"]["langs"][lang]["avg_chars"], 200)

    def test_category_breakdown_counts_first_tries_and_repairs(self):
        cats = {"a": "math", "b": "math", "c": "strings", "d": "strings"}
        s = report.summarize(self.records(), LANGS4, cats)
        self.assertEqual(s["by_category"]["math"]["nyra"], {"n": 2, "pass_at_1": 1, "pass_within_repairs": 2})
        self.assertEqual(s["by_category"]["strings"]["rust"], {"n": 2, "pass_at_1": 1, "pass_within_repairs": 2})
        self.assertEqual(list(s["by_category"]), ["math", "strings"])
        self.assertIsNone(report.summarize(self.records(), LANGS4)["by_category"])

    def test_thinking_and_cost_statistics_appear_only_when_the_api_reports_them(self):
        plain = report.summarize(self.records(), LANGS4)["langs"]["nyra"]
        self.assertIsNone(plain["avg_reasoning_tokens_first_attempt"])
        self.assertIsNone(plain["avg_reply_tokens_first_attempt"])
        self.assertIsNone(plain["total_cost_usd"])
        recs = self.records()
        for r in recs:
            first = r["attempts"][0]
            first["usage"]["reasoning_tokens"] = 30
            first["usage"]["cost_usd"] = 0.5
        s = report.summarize(recs, LANGS4)["langs"]["nyra"]
        self.assertEqual(s["avg_reasoning_tokens_first_attempt"], 30)
        self.assertEqual(s["avg_reply_tokens_first_attempt"], 50)  # 80 billed - 30 thinking
        self.assertAlmostEqual(s["total_cost_usd"], 0.5 * 4)  # first attempts only: the repair attempts have no cost
        self.assertAlmostEqual(s["avg_cost_per_run_usd"], 0.5)

    def test_a_model_report_has_every_language_and_the_token_headline_in_order(self):
        recs = self.records()
        results = fake_results("vendor/model", recs, LANGS4, {"a": "math", "b": "math", "c": "strings", "d": "strings"})
        md = report.render_markdown(results, {})
        header = next(ln for ln in md.splitlines() if ln.startswith("| Metric |"))
        self.assertEqual([c.strip() for c in header.strip("|").split("|")], ["Metric", "Nyra", "Python", "TypeScript", "Rust"])
        self.assertLess(md.index("**Code tokens, first attempt**"), md.index("**Billed output tokens, first attempt**"))
        self.assertNotIn("of which thinking", md)  # not reported, not shown
        self.assertNotIn("Cost per run", md)
        for section in ("## Nyra against each other language", "## Same runs, right on the first try in all 4 languages",
                        "## Per category", "## How first attempts failed", "## Per task"):
            self.assertIn(section, md)
        self.assertIn("| Python | 1 |", md)
        self.assertTrue(md.isascii())
        self.assertIn("Node.js 25.2.1", md)
        self.assertIn("rustc 1.99.0", md)

    def test_thinking_and_cost_rows_are_shown_when_reported(self):
        recs = self.records()
        for r in recs:
            r["attempts"][0]["usage"].update(reasoning_tokens=30, cost_usd=0.01)
        md = report.render_markdown(fake_results("vendor/model", recs, LANGS4), {})
        for row in ("of which thinking, first attempt", "Reply tokens without thinking", "Cost per run (as billed)"):
            self.assertIn(row, md)

    def test_two_languages_still_render_without_the_all_language_table(self):
        recs = [r for r in self.records() if r["lang"] in ("nyra", "python")]
        md = report.render_markdown(fake_results("m", recs, ["nyra", "python"]), {})
        self.assertIn("## Nyra against each other language", md)
        self.assertNotIn("Same runs, right on the first try in all", md)

    def test_a_single_language_has_no_comparison(self):
        recs = [r for r in self.records() if r["lang"] == "python"]
        md = report.render_markdown(fake_results("m", recs, ["python"]), {})
        self.assertNotIn("against each other language", md)
        self.assertIn("**pass@1**", md)

    def test_language_columns_follow_the_standard_order(self):
        self.assertEqual(report.ordered_langs({"rust", "python", "zig", "nyra"}), ["nyra", "python", "rust", "zig"])
        self.assertEqual(report.display("typescript"), "TypeScript")
        self.assertEqual(report.display("zig"), "zig")


class ModelComparison(unittest.TestCase):
    def results(self, model, first_try, **run_fields):
        return fake_results(model, four_language_records(first_try), LANGS4, **run_fields)

    def two_models(self):
        everything = {(t, lang): True for t in ("a", "b") for lang in LANGS4}
        some = dict(everything)
        some[("a", "nyra")] = False
        some[("b", "rust")] = False
        return [self.results("vendor/strong", everything, spent_usd=1.5), self.results("vendor/weak", some)]

    def test_one_row_per_model_and_one_column_per_language(self):
        md = report.render_comparison(self.two_models())
        self.assertTrue(md.isascii())
        for heading in ("## First-try success (pass@1)", "## Success within 3 repairs", "## Tokens of the first attempt",
                        "## Models"):
            self.assertIn(heading, md)
        self.assertIn("| Model | Nyra | Python | TypeScript | Rust |", md)
        self.assertIn("| vendor/strong | 100% (2/2) | 100% (2/2) | 100% (2/2) | 100% (2/2) |", md)
        self.assertIn("| vendor/weak | 50% (1/2) | 100% (2/2) | 100% (2/2) | 50% (1/2) |", md)
        self.assertNotIn("Cost per run", md)

    def test_the_token_cell_is_code_tokens_with_billed_tokens_in_brackets(self):
        md = report.render_comparison(self.two_models())
        self.assertIn("| vendor/strong | 50 (80) | 40 (60) | 60 (90) | 100 (120) |", md)

    def test_costs_and_spent_totals_appear_when_reported(self):
        models = self.two_models()
        for results in models:
            for rec in results["records"]:
                rec["attempts"][0]["usage"]["cost_usd"] = 0.25
            results["summary"] = report.summarize(results["records"], LANGS4)
        md = report.render_comparison(models)
        self.assertIn("## Cost per run (as billed)", md)
        self.assertIn("$0.2500", md)
        self.assertIn("$1.50", md)  # the total of the strong model, from the run metadata
        self.assertIn("| vendor/weak | vendor/weak-20260101 | yes | - |", md)

    def test_mock_incomplete_and_mismatched_runs_are_flagged(self):
        models = self.two_models()
        md = report.render_comparison(models)
        self.assertNotIn("MOCK", md)
        self.assertNotIn("INCOMPLETE", md)
        models[1]["run"]["complete"] = False
        models[1]["run"]["tasks_sha256"] = "other"
        models[0]["run"]["mock"] = True
        md = report.render_comparison(models)
        self.assertIn("MOCK RUN", md)
        self.assertIn("INCOMPLETE", md)
        self.assertIn("differ in task set", md)
        self.assertIn("| vendor/weak | vendor/weak-20260101 | NO |", md)

    def test_models_with_different_languages_get_dashes(self):
        a = self.results("m/a", {(t, lang): True for t in ("a",) for lang in LANGS4})
        recs = [r for r in four_language_records({("a", lang): True for lang in LANGS4}) if r["lang"] in ("python", "rust")]
        b = fake_results("m/b", recs, ["python", "rust"])
        md = report.render_comparison([a, b])
        self.assertIn("| m/b | - | 100% (1/1) | - | 100% (1/1) |", md)

    def test_an_empty_comparison_does_not_crash(self):
        self.assertIn("No results", report.render_comparison([]))


class Publishing(unittest.TestCase):
    CATS = {"a": "math", "b": "strings", "c": "strings", "d": "patterns"}

    def results(self, model, table):
        recs = four_language_records({(t, lang): bool(table[t][i]) for t in table for i, lang in enumerate(LANGS4)})
        for r in recs:  # things that must never be published
            for a in r["attempts"]:
                a.update(reply="REPLY-SECRET", code="CODE-SECRET", feedback="FEEDBACK-SECRET")
                a["result"]["stdout"] = "STDOUT-SECRET"
        return fake_results(model, recs, LANGS4, self.CATS)

    def two(self, **second_run_fields):
        """Two models; `second_run_fields` change the run metadata of the second one."""
        strong = self.results("vendor/strong", {"a": (1, 1, 1, 1), "b": (1, 1, 1, 1), "c": (0, 1, 1, 1), "d": (0, 0, 1, 1)})
        weak = self.results("vendor/weak", {"a": (1, 1, 1, 1), "b": (0, 1, 1, 1), "c": (0, 0, 1, 1), "d": (0, 0, 0, 1)})
        weak["run"].update(second_run_fields)
        return strong, weak

    def write(self, directory, *results_list):
        paths = []
        for i, results in enumerate(results_list):
            path = Path(directory) / f"result-{i}.json"
            path.write_text(json.dumps(results), encoding="utf-8")
            paths.append(path)
        return paths

    def test_the_summary_has_the_sections_and_the_numbers(self):
        with tempfile.TemporaryDirectory() as tmp:
            paths = self.write(tmp, *self.two())
            md, data, written = publish.publish(paths, "2026-10-demo", Path(tmp) / "pub")
            self.assertEqual(sorted(p.name for p in written), ["2026-10-demo.json", "2026-10-demo.md"])
            self.assertEqual(written[0].read_text(encoding="utf-8"), md)
            on_disk = json.loads(written[1].read_text(encoding="utf-8"))
        self.assertEqual(on_disk, json.loads(json.dumps(data)))
        for heading in ("# Nyra benchmark results: 2026-10-demo", "## What was measured", "## First-try success (pass@1)",
                        "## Success within 3 repairs", "## Tokens of the first attempt", "## Nyra code tokens relative",
                        "## Per model", "### vendor/strong", "### vendor/weak", "#### Per category",
                        "## Notable failures", "## How to read this"):
            self.assertIn(heading, md)
        self.assertIn("vendor/strong", data["models"])
        self.assertEqual(data["models"]["vendor/strong"]["langs"]["nyra"]["pass_at_1"], 2)
        self.assertEqual(data["run"]["langs"], LANGS4)
        self.assertEqual(data["run"]["harness_commits"], ["abc1234"])
        self.assertTrue(md.isascii())

    def test_no_prompt_reply_program_or_output_is_published(self):
        with tempfile.TemporaryDirectory() as tmp:
            md, data, _ = publish.publish(self.write(tmp, *self.two()), "x", Path(tmp) / "pub", write=False)
        text = md + json.dumps(data)
        for secret in ("REPLY-SECRET", "CODE-SECRET", "FEEDBACK-SECRET", "STDOUT-SECRET", "PROMPT-a", "SYSTEM-nyra"):
            self.assertNotIn(secret, text)

    def test_sources_are_named_with_their_checksums(self):
        with tempfile.TemporaryDirectory() as tmp:
            paths = self.write(tmp, *self.two())
            _, data, _ = publish.publish(paths, "x", Path(tmp) / "pub", write=False)
            expected = [hashlib.sha256(p.read_bytes()).hexdigest() for p in paths]
        self.assertEqual([s["sha256"] for s in data["sources"]], expected)
        self.assertEqual([s["file"] for s in data["sources"]], ["result-0.json", "result-1.json"])

    def test_the_nyra_ratio_table_averages_the_runs_both_got_right(self):
        with tempfile.TemporaryDirectory() as tmp:
            md, data, _ = publish.publish(self.write(tmp, *self.two()), "x", Path(tmp) / "pub", write=False)
        # Nyra got only tasks a and b right first try, so n=2 against every language; code tokens are
        # nyra 50, python 40 (1.25x), typescript 60 (0.83x), rust 100 (0.50x) in every run
        self.assertIn("| vendor/strong | 1.25x (n=2) | 0.83x (n=2) | 0.50x (n=2) |", md)

    def test_mock_results_are_refused_unless_allowed_and_then_marked(self):
        strong, weak = self.two()
        strong["run"]["mock"] = True
        with tempfile.TemporaryDirectory() as tmp:
            paths = self.write(tmp, strong, weak)
            with self.assertRaises(publish.PublishError) as cm:
                publish.publish(paths, "x", Path(tmp) / "pub")
            self.assertIn("mock", str(cm.exception))
            self.assertFalse((Path(tmp) / "pub").exists())
            md, data, _ = publish.publish(paths, "x", Path(tmp) / "pub", allow_mock=True, write=False)
        self.assertIn("MOCK RESULTS", md)
        self.assertTrue(data["mock"])

    def test_incomplete_runs_are_refused_unless_allowed_and_then_marked(self):
        with tempfile.TemporaryDirectory() as tmp:
            paths = self.write(tmp, *self.two(complete=False))
            with self.assertRaises(publish.PublishError) as cm:
                publish.publish(paths, "x", Path(tmp) / "pub")
            self.assertIn("stopped early", str(cm.exception))
            md, data, _ = publish.publish(paths, "x", Path(tmp) / "pub", allow_incomplete=True, write=False)
        self.assertIn("INCOMPLETE", md)
        self.assertFalse(data["complete"])
        self.assertTrue(any("--allow-incomplete" in n for n in data["notes"]))

    def test_results_measured_differently_are_refused_unless_allowed(self):
        for field, value in (("tasks_sha256", "o" * 64), ("samples", 5), ("repairs", 1), ("langs", ["nyra", "python"]),
                             ("spec", {"sha256": "x" * 64}), ("nyra", {"version": "nyra 0.3.0"})):
            with tempfile.TemporaryDirectory() as tmp:
                paths = self.write(tmp, *self.two(**{field: value}))
                with self.assertRaises(publish.PublishError, msg=field) as cm:
                    publish.publish(paths, "x", Path(tmp) / "pub")
                self.assertIn("not comparable", str(cm.exception))
                md, _, _ = publish.publish(paths, "x", Path(tmp) / "pub", allow_mismatch=True, write=False)
                self.assertIn("published with --allow-mismatch", md)

    def test_the_same_model_twice_is_refused(self):
        strong, _ = self.two()
        with tempfile.TemporaryDirectory() as tmp:
            paths = self.write(tmp, strong, strong)
            with self.assertRaises(publish.PublishError) as cm:
                publish.publish(paths, "x", Path(tmp) / "pub")
        self.assertIn("same model", str(cm.exception))

    def test_names_files_and_overwriting(self):
        with tempfile.TemporaryDirectory() as tmp:
            paths = self.write(tmp, *self.two())
            for bad in ("", "../x", "a b", ".hidden", "x/y"):
                with self.assertRaises(publish.PublishError, msg=bad):
                    publish.publish(paths, bad, Path(tmp) / "pub")
            publish.publish(paths, "v1", Path(tmp) / "pub")
            with self.assertRaises(publish.PublishError) as cm:
                publish.publish(paths, "v1", Path(tmp) / "pub")
            self.assertIn("--overwrite", str(cm.exception))
            publish.publish(paths, "v1", Path(tmp) / "pub", overwrite=True)
            with self.assertRaises(publish.PublishError):
                publish.publish([Path(tmp) / "missing.json"], "v2", Path(tmp) / "pub")
            (Path(tmp) / "junk.json").write_text('{"hello": 1}')
            with self.assertRaises(publish.PublishError) as cm:
                publish.publish([Path(tmp) / "junk.json"], "v2", Path(tmp) / "pub")
            self.assertIn("not a benchmark result", str(cm.exception))
            (Path(tmp) / "bad.json").write_text("not json")
            with self.assertRaises(publish.PublishError):
                publish.publish([Path(tmp) / "bad.json"], "v2", Path(tmp) / "pub")

    def test_a_compare_index_stands_for_its_result_files(self):
        with tempfile.TemporaryDirectory() as tmp:
            paths = self.write(tmp, *self.two())
            index = Path(tmp) / "run-compare.json"
            index.write_text(json.dumps({"models": [{"model": "vendor/strong", "file": paths[0].name},
                                                     {"model": "vendor/weak", "file": paths[1].name}]}))
            _, data, _ = publish.publish([index], "x", Path(tmp) / "pub", write=False)
            self.assertEqual(sorted(data["models"]), ["vendor/strong", "vendor/weak"])
            # the same file named twice (directly and through the index) counts once
            _, data, _ = publish.publish([index] + paths, "x", Path(tmp) / "pub", write=False)
            self.assertEqual(len(data["sources"]), 2)
            index.write_text(json.dumps({"models": [{"model": "m", "file": "../escape.json"}]}))
            with self.assertRaises(publish.PublishError):
                publish.publish([index], "x", Path(tmp) / "pub", write=False)

    def test_notable_failures_list_compiler_bugs_first_then_runs_that_never_passed(self):
        rec = ReportSummary.rec
        nyra_fail = rec("hard", "nyra", False, False, kind="compile_error", attempts=4,
                        errors=[{"code": "E0201", "message": "undefined variable `cout`"}])
        nyra_fail["attempts"][-1]["result"]["errors"] = [{"code": "E0203", "message": "type mismatch | expected `int`"}]
        rust_fail = rec("hard", "rust", False, False, kind="compile_error", attempts=2)
        rust_fail["attempts"][-1]["result"]["stderr"] = "warning: x\nerror[E0425]: cannot find value `y`\nmore"
        ts_fail = rec("hard", "typescript", False, False, kind="runtime_error", attempts=2)
        ts_fail["attempts"][-1]["result"]["stderr"] = "main.ts:3\n\nTypeError: x is not a function"
        bug = rec("fine", "rust", False, True, kind="toolchain_error", attempts=2)
        bug["attempts"][0]["result"]["stderr"] = "error: linking with `cc` failed"
        wrong = rec("wrong", "python", False, False, kind="wrong_output", attempts=3)
        ok = rec("easy", "python", True, True)
        results = fake_results("m", [nyra_fail, rust_fail, ts_fail, bug, wrong, ok], LANGS4)
        entries = publish.notable_failures(results)
        self.assertEqual([(e["lang"], e["task"], e["outcome"]) for e in entries],
                         [("rust", "fine", "compiler or toolchain bug"), ("nyra", "hard", "never passed"),
                          ("python", "wrong", "never passed"), ("typescript", "hard", "never passed"),
                          ("rust", "hard", "never passed")])
        by = {(e["lang"], e["task"]): e for e in entries}
        self.assertEqual(by["nyra", "hard"]["error_codes"], ["E0201", "E0203"])
        self.assertEqual(by["nyra", "hard"]["message"], "E0203: type mismatch | expected `int`")
        self.assertEqual(by["rust", "hard"]["message"], "error[E0425]: cannot find value `y`")
        self.assertEqual(by["typescript", "hard"]["message"], "TypeError: x is not a function")
        self.assertEqual(by["python", "wrong"]["message"], "the program ran but printed the wrong output")
        self.assertEqual(by["rust", "fine"]["message"], "error: linking with `cc` failed")
        self.assertEqual(by["python", "wrong"]["attempts"], 3)
        self.assertEqual(len(publish.notable_failures(results, limit=2)), 2)

    def test_the_output_of_a_real_multi_model_run_is_publishable(self):
        # run.py (here against a fake OpenRouter on this machine) -> the compare index -> publish.py
        tasks = run.load_tasks()
        with mock.patch.dict(os.environ, {providers.OPENROUTER_KEY_ENV: API_KEY}), \
                _fake_openrouter(["good/model", "weak/model"], tasks) as server, tempfile.TemporaryDirectory() as out:
            code, _, stderr = _run_main("--provider", "openrouter", "--base-url", server.url + "/api/v1", "--langs",
                                        "python", "--tasks", "fizzbuzz,gcd_pairs", "-q", "--jobs", "2",
                                        "--models", "good/model,weak/model", "--out", out)
            self.assertEqual(code, 0, stderr)
            md, data, written = publish.publish([str(next(Path(out).glob("*compare.json")))], "demo",
                                                Path(out) / "published", write=True)
            self.assertEqual({p.name for p in written}, {"demo.md", "demo.json"})
        self.assertEqual(sorted(data["models"]), ["good/model", "weak/model"])
        self.assertIn("good/model", md)
        self.assertIn("weak/model", md)

    def test_pipes_in_messages_cannot_break_a_table(self):
        self.assertEqual(publish._cell("a | b\nc"), "a \\| b c")

    def test_common_nyra_errors_and_hardest_tasks(self):
        rec = ReportSummary.rec
        recs = [rec("t1", "nyra", False, True, kind="compile_error", attempts=2, errors=[{"code": "E0201", "message": "m1"}]),
                rec("t2", "nyra", False, True, kind="compile_error", attempts=2,
                    errors=[{"code": "E0201", "message": "m2"}, {"code": "E0203", "message": "m3"}]),
                rec("t1", "python", True, True), rec("t2", "python", False, True, kind="wrong_output", attempts=2)]
        a = fake_results("a/a", recs, ["nyra", "python"], {"t1": "math", "t2": "strings"})
        b = fake_results("b/b", recs, ["nyra", "python"], {"t1": "math", "t2": "strings"})
        errors = publish.common_nyra_errors([a, b])
        self.assertEqual([(e["code"], e["count"]) for e in errors], [("E0201", 4), ("E0203", 2)])
        self.assertEqual(errors[0]["message"], "m1")
        hard = publish.hardest_tasks([a, b])
        self.assertEqual([h["task"] for h in hard], ["t1", "t2"])  # both fail for nyra every time; the order is by task
        self.assertEqual(hard[0]["rates"], {"nyra": [0, 2], "python": [2, 2]})
        self.assertEqual(hard[1]["category"], "strings")
        self.assertEqual(publish.hardest_tasks([fake_results("c/c", [rec("e", "python", True, True)], ["python"])]), [])

    def test_the_command_line(self):
        with tempfile.TemporaryDirectory() as tmp:
            paths = self.write(tmp, *self.two())
            out, err = io.StringIO(), io.StringIO()
            with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
                code = publish.main([str(p) for p in paths] + ["--name", "cli", "--out", str(Path(tmp) / "pub")])
            self.assertEqual(code, 0, err.getvalue())
            self.assertIn("wrote", out.getvalue())
            self.assertTrue((Path(tmp) / "pub" / "cli.md").is_file())
            out, err = io.StringIO(), io.StringIO()
            with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
                code = publish.main([str(p) for p in paths] + ["--name", "cli", "--out", str(Path(tmp) / "pub")])
            self.assertEqual(code, 2)
            self.assertIn("already exists", err.getvalue())
            out, err = io.StringIO(), io.StringIO()
            with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
                code = publish.main([str(p) for p in paths] + ["--name", "preview", "--stdout",
                                                                "--out", str(Path(tmp) / "pub")])
            self.assertEqual(code, 0)
            self.assertIn("# Nyra benchmark results: preview", out.getvalue())
            self.assertFalse((Path(tmp) / "pub" / "preview.md").exists())

    def test_raw_results_are_ignored_by_git_and_published_ones_are_not(self):
        ignore = (BENCH_DIR / "results" / ".gitignore").read_text(encoding="utf-8")
        lines = [ln.strip() for ln in ignore.splitlines() if ln.strip() and not ln.startswith("#")]
        self.assertIn("*", lines)  # everything in bench/results stays local ...
        self.assertIn("!.gitignore", lines)  # ... except this file
        self.assertTrue((BENCH_DIR / "published").is_dir())
        self.assertFalse((BENCH_DIR / "published" / ".gitignore").exists())


class CountingPolicy(unittest.TestCase):
    """Counting code tokens costs money with some providers: the runner asks about first attempts only."""

    def run_with(self, count_all_attempts):
        counted = []

        class Counting(providers.Provider):
            name, default_model = "c", "c"

            def complete(self, system, messages, meta):
                text = "```python\nprint(%d)\n```" % (1 if meta["attempt"] > 1 else 0)
                return providers.Reply(text, providers.Usage(1, 1))

            def count_tokens(self, text):
                counted.append(text)
                return 3

        Counting.count_all_attempts = count_all_attempts
        rec = run.run_one(_task("1\n"), run.PythonLang(timeout=5), 0,
                          run.RunContext(provider=Counting(), repairs=2, count_tokens=True))
        return rec, counted

    def test_a_provider_that_bills_for_counting_is_asked_about_first_attempts_only(self):
        self.assertFalse(providers.OpenRouterProvider.count_all_attempts)
        self.assertTrue(providers.AnthropicProvider.count_all_attempts)
        rec, counted = self.run_with(False)
        self.assertEqual(rec["attempts_used"], 2)
        self.assertEqual([a["code_tokens"] for a in rec["attempts"]], [3, None])
        self.assertEqual(counted, ["print(0)"])

    def test_a_free_counter_is_asked_about_every_attempt(self):
        rec, counted = self.run_with(True)
        self.assertEqual([a["code_tokens"] for a in rec["attempts"]], [3, 3])
        self.assertEqual(counted, ["print(0)", "print(1)"])

    def test_counting_can_be_switched_off(self):
        counted = []

        class Counting(providers.Provider):
            name, default_model = "c", "c"

            def complete(self, system, messages, meta):
                return providers.Reply("```python\nprint(1)\n```", providers.Usage(1, 1))

            def count_tokens(self, text):
                counted.append(text)
                return 3

        rec = run.run_one(_task("1\n"), run.PythonLang(timeout=5), 0,
                          run.RunContext(provider=Counting(), repairs=0, count_tokens=False))
        self.assertEqual((counted, rec["attempts"][0]["code_tokens"]), ([], None))


@needs_nyra
class MockModels(unittest.TestCase):
    """Several mock models in one run: different 'models' for the comparison tables, no API."""

    ARGS = ("--provider", "mock", "--langs", "nyra,python", "--tasks", "fizzbuzz,gcd_pairs,grade_letters", "-q")

    def run_models(self, models, *extra):
        with tempfile.TemporaryDirectory() as out:
            code, stdout, stderr = _run_main(*self.ARGS, "--models", models, "--out", out, *extra)
            files = sorted(p.name for p in Path(out).iterdir())
            loaded = {}
            for p in Path(out).glob("*.json"):
                data = json.loads(p.read_text(encoding="utf-8"))
                loaded["index" if "records" not in data else data["run"]["provider"]["model"]] = data
            compare = next(Path(out).glob("*compare.md")).read_text(encoding="utf-8") if models.count(",") else None
        return code, stdout, stderr, files, loaded, compare

    def test_each_model_gets_its_result_files_and_the_run_a_comparison(self):
        code, stdout, stderr, files, loaded, compare = self.run_models("mock,mock-flaky,mock-wrong")
        self.assertEqual(code, 0, stderr)
        self.assertEqual(len(files), 9)  # three results x (json, md), the comparison (md, json) and latest.md
        self.assertEqual(sorted(loaded), ["index", "mock", "mock-flaky", "mock-wrong"])
        self.assertEqual(loaded["mock"]["summary"]["langs"]["python"]["pass_at_1"], 3)
        self.assertEqual(loaded["mock-wrong"]["summary"]["langs"]["python"]["pass_at_1"], 0)
        self.assertEqual(loaded["mock-wrong"]["summary"]["langs"]["python"]["pass_within_repairs"], 3)
        flaky = loaded["mock-flaky"]["summary"]["langs"]
        persona = providers.MockProvider("mock-flaky", reference=lambda lang, task: None)
        for lang in ("nyra", "python"):  # the first try fails exactly where the mock breaks it on purpose
            unbroken = sum(1 for t in ("fizzbuzz", "gcd_pairs", "grade_letters") if persona.defect_for(lang, t) is None)
            self.assertEqual(flaky[lang]["pass_at_1"], unbroken, lang)
            self.assertEqual(flaky[lang]["pass_within_repairs"], 3, lang)
        self.assertEqual([m["model"] for m in loaded["index"]["models"]], ["mock", "mock-flaky", "mock-wrong"])
        for model in ("mock", "mock-flaky", "mock-wrong"):
            self.assertIn(f"| {model} |", compare)
        self.assertIn("MOCK RUN", compare)
        self.assertIn("=== model 2 of 3: mock-flaky ===", stdout)
        self.assertIn("mock-wrong: pass@1 Nyra 0/3, Python 0/3", stdout)

    def test_a_single_model_prints_its_full_report_and_writes_no_comparison(self):
        code, stdout, _, files, loaded, _ = self.run_models("mock")
        self.assertEqual(code, 0)
        self.assertEqual(sorted(loaded), ["mock"])
        self.assertIn("## Headline", stdout)
        self.assertFalse([f for f in files if "compare" in f])

    def test_the_comparison_is_there_for_an_interrupted_or_partial_set_too(self):
        # models run one after the other: with a model that cannot finish, the others still land in the comparison
        class Dead(providers.MockProvider):
            def __init__(self, model=None, **options):
                super().__init__("mock", **options)
                self.model = model

            def complete(self, system, messages, meta):
                raise providers.ProviderError("HTTP 404: no such model", fatal=True)

        real = providers.make_provider

        def make(name, model=None, **options):
            return Dead(model, **options) if model == "mock-dead" else real(name, model, **options)

        with mock.patch.object(providers, "make_provider", make):
            code, _, stderr, files, loaded, compare = self.run_models("mock,mock-dead,mock-wrong")
        self.assertEqual(code, 2)
        self.assertIn("no run finished for mock-dead", stderr)
        self.assertEqual(sorted(loaded), ["index", "mock", "mock-wrong"])
        self.assertNotIn("mock-dead", compare)

    def test_repairs_zero_makes_the_flawed_models_fail_the_self_test(self):
        code, _, stderr, _, loaded, _ = self.run_models("mock,mock-wrong", "--repairs", "0")
        self.assertEqual(code, 1)
        self.assertIn("self-test", stderr)
        self.assertEqual(loaded["mock"]["summary"]["langs"]["python"]["pass_within_repairs"], 3)

    def test_option_errors(self):
        cases = (
            (("--models", "mock", "--model", "mock"), "--model or --models, not both"),
            (("--models", "mock-bogus"), "unknown mock model 'mock-bogus'"),
            (("--models", "default"), "OpenRouter list"),
            (("--models", "mock,mock"), "listed more than once"),
            (("--models", " , "), "no model ids given"),
        )
        for extra, message in cases:
            code, _, stderr = _run_main("--provider", "mock", "--langs", "python", "--tasks", "fizzbuzz", "--dry-run", *extra)
            self.assertEqual(code, 2, extra)
            self.assertIn(message, stderr)
        code, _, stderr = _run_main("--provider", "openrouter", "--langs", "python", "--dry-run")
        self.assertEqual(code, 2)
        self.assertIn("no default model", stderr)
        self.assertIn("--models", stderr)

    def test_effort_levels_depend_on_the_provider(self):
        code, _, stderr = _run_main("--provider", "anthropic", "--effort", "none", "--langs", "python", "--dry-run")
        self.assertEqual(code, 2)
        self.assertIn("OpenRouter reasoning levels", stderr)
        code, _, _ = _run_main("--provider", "anthropic", "--effort", "high", "--langs", "python", "--tasks", "fizzbuzz",
                               "--dry-run")
        self.assertEqual(code, 0)

    def test_default_models_expand_to_the_configured_list(self):
        ids = modelsmod.default_model_ids()
        listing = [{"id": mid, "name": mid, "pricing": {"prompt": "0.000001", "completion": "0.000002"}} for mid in ids]
        with mock.patch.object(modelsmod, "fetch_models", return_value=listing):
            code, stdout, _ = _run_main("--provider", "openrouter", "--models", "default", "--langs", "python",
                                        "--tasks", "fizzbuzz", "--dry-run")
        self.assertEqual(code, 0)
        for mid in ids:
            self.assertIn(f"{mid}: about $", stdout)
        self.assertIn(f"{4 * len(ids)} in total", stdout)


@needs_nyra
@needs_node
class MockPipelineFourLanguages(NeedsRust):
    TASKS = "fizzbuzz,gcd_pairs,roman_numerals"

    def run_main_out(self, *extra, tasks=None):
        with tempfile.TemporaryDirectory() as out:
            code, stdout, stderr = _run_main("--provider", "mock", "--tasks", tasks or self.TASKS, "--out", out, "-q", *extra)
            results = json.loads(next(Path(out).glob("*.json")).read_text(encoding="utf-8"))
        return code, stdout, stderr, results

    def test_all_four_languages_run_and_pass(self):
        code, stdout, stderr, results = self.run_main_out()
        self.assertEqual(code, 0, stderr)
        self.assertEqual(results["run"]["langs"], LANGS4)
        self.assertEqual(len(results["records"]), 12)
        self.assertTrue(all(r["status"] == "pass" and r["first_try"] for r in results["records"]))
        for lang in LANGS4:
            self.assertEqual(results["summary"]["langs"][lang]["pass_at_1"], 3)
        self.assertEqual(set(results["run"]["system_prompts"]), set(LANGS4))
        self.assertIn("| Metric | Nyra | Python | TypeScript | Rust |", stdout)
        self.assertIn("Nyra against each other language", stdout)

    def test_the_toolchains_are_recorded_without_local_paths(self):
        _, _, _, results = self.run_main_out("--langs", "typescript,rust", tasks="fizzbuzz")
        run_meta = results["run"]
        self.assertRegex(run_meta["node"]["version"], r"^v\d+\.\d+\.\d+")
        self.assertIn("--experimental-transform-types", run_meta["node"]["flags"])
        self.assertTrue(run_meta["rust"]["version"].startswith("rustc "))
        self.assertTrue(run_meta["rust"]["command"].startswith("rustc"))
        self.assertIn("--edition 2021", run_meta["rust"]["flags"])
        self.assertIn("-O", run_meta["rust"]["flags"].split())
        self.assertIsNone(run_meta["nyra"])
        self.assertNotIn(os.path.expanduser("~"), json.dumps(run_meta))
        self.assertEqual(results["schema"], 3)

    def test_each_defect_is_repaired_in_typescript_and_rust(self):
        expected_kind = {"no_code": "no_code", "syntax": "compile_error", "runtime": "runtime_error",
                         "wrong": "wrong_output"}
        for defect, kind in expected_kind.items():
            with self.subTest(defect=defect):
                code, _, stderr, results = self.run_main_out("--langs", "typescript,rust", "--mock-flaky", defect,
                                                             tasks="fizzbuzz")
                self.assertEqual(code, 0, stderr)
                for rec in results["records"]:
                    self.assertEqual((rec["status"], rec["attempts_used"]), ("pass", 2), rec["lang"])
                    first = rec["attempts"][0]
                    self.assertEqual(first["result"]["kind"], kind, rec["lang"])
                    self.assertTrue(first["feedback"])
                    self.assertNotIn("nyra-bench-", first["feedback"])

    def test_a_typescript_only_run_needs_neither_nyra_nor_rust(self):
        with mock.patch.object(run, "find_nyra", side_effect=AssertionError("nyra is not needed")), \
                mock.patch.object(run, "_find_rust_tool", side_effect=AssertionError("rust is not needed")):
            code, _, stderr, results = self.run_main_out("--langs", "typescript", tasks="fizzbuzz,binary_search")
        self.assertEqual(code, 0, stderr)
        self.assertEqual(len(results["records"]), 2)  # no Nyra, so no version limit: the 0.3 task runs too

    def test_a_nyra_run_limits_every_language_to_the_tasks_nyra_can_express(self):
        code, stdout, _ = _run_main("--provider", "mock", "--langs", "nyra,typescript,rust", "--tasks",
                                    "fizzbuzz,binary_search", "--max-version", "0.2", "--dry-run")
        self.assertEqual(code, 0)
        self.assertIn("fizzbuzz", stdout)
        self.assertIn("skipped binary_search: needs Nyra 0.3", stdout)


class VerifyLanguages(unittest.TestCase):
    """verify.py must be able to fail for TypeScript and Rust references too."""

    @classmethod
    def setUpClass(cls):
        if not NODE or rust_toolchain() is None:
            raise unittest.SkipTest("needs Node.js and a Rust toolchain")

    def setUp(self):
        import verify
        self.verify = verify
        self.tmp = tempfile.TemporaryDirectory()
        self.sol = Path(self.tmp.name)
        for lang in ("python", "nyra", "typescript", "rust"):
            (self.sol / lang).mkdir()
        (self.sol / "python" / "t.py").write_text("print(42)\n")
        patcher = mock.patch.object(run, "SOLUTIONS_DIR", self.sol)
        patcher.start()
        self.addCleanup(patcher.stop)
        self.addCleanup(self.tmp.cleanup)
        self.args = SimpleNamespace(timeout=10, write=False, strict=False)
        self.extra = {"typescript": run.TypeScriptLang(timeout=10), "rust": rust_toolchain()}

    def check(self, extra=None):
        task = run.Task("t", "T", "prompt", "42\n", "0.1", "math", "easy", Path("t.json"))
        return self.verify.check_task(task, self.args, {}, None, self.extra if extra is None else extra)

    def good(self):
        (self.sol / "typescript" / "t.ts").write_text("console.log(42);\n")
        (self.sol / "rust" / "t.rs").write_text('fn main() {\n    println!("42");\n}\n')

    def test_good_references_pass_and_are_noted(self):
        self.good()
        res = self.check()
        self.assertEqual(res["problems"], [])
        self.assertIn("typescript, rust", res["notes"])

    def test_a_missing_reference_is_a_problem_whatever_the_version(self):
        self.good()
        (self.sol / "rust" / "t.rs").unlink()
        res = self.check()
        self.assertEqual(len(res["problems"]), 1)
        self.assertIn("missing bench/solutions/rust/t.rs", res["problems"][0])
        (self.sol / "typescript" / "t.ts").unlink()
        self.assertEqual(len(self.check()["problems"]), 2)

    def test_wrong_output_and_broken_programs_are_reported_per_language(self):
        self.good()
        (self.sol / "typescript" / "t.ts").write_text("console.log(41);\n")
        (self.sol / "rust" / "t.rs").write_text("fn main() {\n    println!(oops);\n}\n")
        problems = self.check()["problems"]
        self.assertEqual(len(problems), 2)
        self.assertIn("TypeScript reference: wrong_output: first difference on line 1", problems[0])
        self.assertIn("Rust reference: compile_error", problems[1])

    def test_only_the_requested_languages_are_checked(self):
        self.good()
        (self.sol / "rust" / "t.rs").unlink()
        self.assertEqual(self.check({"typescript": self.extra["typescript"]})["problems"], [])

class VerifyCommandLine(unittest.TestCase):
    def test_skip_leaves_toolchains_out_and_says_so(self):
        import verify
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = verify.main(["--skip", "nyra,typescript,rust", "--tasks", "fizzbuzz", "--timeout", "20"])
        self.assertEqual(code, 0, err.getvalue())
        self.assertIn("not checked here (--skip): nyra, rust, typescript", out.getvalue())
        self.assertNotIn("typescript;", out.getvalue())
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(err):
            self.assertEqual(verify.main(["--skip", "cobol"]), 2)
        self.assertIn("unknown language", err.getvalue())
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(err):
            self.assertEqual(verify.main(["--tasks", "no_such_task", "--skip", "nyra,typescript,rust"]), 2)

    def test_max_version_must_be_a_version(self):
        import verify
        err = io.StringIO()
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(err):
            self.assertEqual(verify.main(["--max-version", "next", "--skip", "nyra,typescript,rust"]), 2)
        self.assertIn("not a version", err.getvalue())

    @needs_nyra
    def test_max_version_replaces_the_compiler_version(self):
        # while Cargo.toml still has the old version, the references of the new one can be checked
        import verify
        seen = []

        def fake_check(task, args, nyra_langs, version, extra_langs=None):
            seen.append(version)
            return {"id": task.id, "problems": [], "notes": [], "new_expected": None}

        out = io.StringIO()
        with mock.patch.object(verify, "check_task", fake_check), contextlib.redirect_stdout(out), \
                contextlib.redirect_stderr(io.StringIO()):
            code = verify.main(["--max-version", "0.7", "--skip", "typescript,rust", "--tasks", "fizzbuzz"])
        self.assertEqual((code, seen), (0, [(0, 7)]))
        self.assertIn("as Nyra 0.7 (--max-version), the compiler says nyra ", out.getvalue())
        seen.clear()
        with mock.patch.object(verify, "check_task", fake_check), contextlib.redirect_stdout(io.StringIO()), \
                contextlib.redirect_stderr(io.StringIO()):
            verify.main(["--skip", "typescript,rust", "--tasks", "fizzbuzz"])
        self.assertEqual(seen, [run.NyraLang(NYRA).version()])

    @unittest.skipUnless(NODE, "Node.js is not installed")
    def test_typescript_is_checked_unless_skipped(self):
        import verify
        out = io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(io.StringIO()):
            code = verify.main(["--skip", "nyra,rust", "--tasks", "fizzbuzz,gcd_pairs"])
        self.assertEqual(code, 0)
        self.assertIn("2 with a TypeScript reference", out.getvalue())
        self.assertIn("(typescript)", out.getvalue())
        self.assertNotIn("rust", out.getvalue().replace("not checked here (--skip): nyra, rust", ""))


# ============================================================ runtime, speed tasks, self-repair


class SpeedTasks(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tasks = [t for t in run.load_tasks() if t.category == report.SPEED_CATEGORY]

    def test_the_speed_tier_exists_and_says_that_time_is_measured(self):
        self.assertTrue(5 <= len(self.tasks) <= 12, len(self.tasks))
        for t in self.tasks:
            self.assertEqual(t.min_version, "0.3", t.id)  # only Nyra features that exist today
            self.assertIn("running time of the program is measured", t.prompt, t.id)
            for lang, ext in (("python", ".py"), ("nyra", ".nyra"), ("typescript", ".ts"), ("rust", ".rs")):
                self.assertTrue((run.SOLUTIONS_DIR / lang / f"{t.id}{ext}").is_file(), f"{t.id}{ext}")

    def test_speed_outputs_stay_exact_in_every_language(self):
        # whole numbers below 2^53, so the JavaScript backend and TypeScript print them exactly
        for t in self.tasks:
            for number in re.findall(r"-?\d+", t.expected_output):
                self.assertLess(abs(int(number)), 2 ** 53, t.id)


class Timing(unittest.TestCase):
    def test_a_passing_program_is_timed_and_the_start_up_is_subtracted(self):
        lang = run.PythonLang(timeout=10, time_runs=3)
        r = lang.evaluate("print(1)\n", _task("1\n"))
        self.assertTrue(r.passed)
        self.assertEqual(len(r.timing["runs_ms"]), 3)
        self.assertEqual(r.timing["median_ms"], round(sorted(r.timing["runs_ms"])[1], 2))
        startup = lang.measured_startup_ms()
        self.assertIsNotNone(startup)
        self.assertAlmostEqual(r.runtime_ms, max(0.0, sorted(r.timing["runs_ms"])[1] - startup), delta=0.02)
        d = r.to_dict()
        self.assertIn("runtime_ms", d)
        self.assertEqual(d["timing"], r.timing)

    def test_the_start_up_is_measured_once(self):
        lang = run.PythonLang(timeout=10, time_runs=1)
        calls = []
        real = run.time_command

        def spy(*a, **kw):
            calls.append(kw["runs"])
            return real(*a, **kw)

        with mock.patch.object(run, "time_command", spy):
            first = lang.startup_ms()
            self.assertEqual(lang.startup_ms(), first)
            lang.evaluate("print(1)\n", _task("1\n"))
        self.assertEqual(calls, [run.STARTUP_RUNS, 1])  # the start-up runs, then one timed run of the program

    def test_failing_programs_and_switched_off_timing_are_not_timed(self):
        r = run.PythonLang(timeout=10, time_runs=2).evaluate("print(2)\n", _task("1\n"))
        self.assertEqual((r.kind, r.runtime_ms, r.timing), ("wrong_output", None, None))
        r = run.PythonLang(timeout=10).evaluate("print(1)\n", _task("1\n"))
        self.assertEqual((r.passed, r.runtime_ms, r.timing), (True, None, None))
        r = run.PythonLang(timeout=10, time_runs=2).evaluate("print(1)\n", _task("1\n"), timed=False)
        self.assertIsNone(r.timing)

    def test_a_timed_run_that_prints_something_else_does_not_count(self):
        code = ("import os\nn = int(open('n').read()) if os.path.exists('n') else 0\n"
                "open('n', 'w').write(str(n + 1))\nprint(1 if n == 0 else 2)\n")
        r = run.PythonLang(timeout=10, time_runs=2).evaluate(code, _task("1\n"))
        self.assertTrue(r.passed)  # the verdict is the judged run's
        self.assertIsNone(r.runtime_ms)
        self.assertIn("different output", r.timing["error"])

    def test_the_toolchain_self_test_is_never_timed(self):
        lang = run.PythonLang(timeout=10, time_runs=2)
        with mock.patch.object(run.Language, "time_result", side_effect=AssertionError("timed")):
            self.assertEqual(lang.preflight(), [])


def speed_records(table, langs, sample=0):
    """table: {task: {lang: (code_tokens, runtime_ms or None)}}; every run passes first try."""
    out = []
    for task, cells in table.items():
        for lang in langs:
            tokens, runtime = cells[lang]
            rec = ReportSummary.rec(task, lang, True, True, code_tokens=tokens, sample=sample)
            rec["attempts"][0]["result"]["runtime_ms"] = runtime
            out.append(rec)
    return out


class RuntimeReport(unittest.TestCase):
    LANGS = ["nyra", "python", "typescript", "rust"]
    CATS = {"s1": "speed", "s2": "speed", "s3": "speed", "m": "math"}

    def table(self):
        # code tokens / runtime: Nyra 200 tokens and 5 ms, Python 500 tokens and 1 s (the owner's example)
        return {
            "s1": {"nyra": (200, 4.0), "python": (500, 900.0), "typescript": (400, 50.0), "rust": (600, 0.2)},
            "s2": {"nyra": (200, 5.0), "python": (500, 1000.0), "typescript": (400, 60.0), "rust": (600, 0.3)},
            "s3": {"nyra": (200, 6.0), "python": (500, 1100.0), "typescript": (400, 70.0), "rust": (600, 0.4)},
            "m": {"nyra": (200, 900.0), "python": (500, 0.0), "typescript": (400, 0.0), "rust": (600, 0.0)},
        }

    def summary(self):
        return report.summarize(speed_records(self.table(), self.LANGS), self.LANGS, self.CATS)

    def test_medians_and_the_efficiency_formula(self):
        eff = self.summary()["efficiency"]
        self.assertEqual((eff["reference"], eff["runs"], eff["speed_runs"]), ("python", 4, 3))
        nyra, py, rust = eff["langs"]["nyra"], eff["langs"]["python"], eff["langs"]["rust"]
        self.assertEqual((nyra["median_code_tokens"], nyra["median_runtime_ms"]), (200, 5.0))  # task m is not timed
        self.assertEqual((py["tokens_factor"], py["runtime_factor"], py["efficiency"]), (1.0, 1.0, 1.0))
        self.assertAlmostEqual(nyra["tokens_factor"], 0.4)
        self.assertAlmostEqual(nyra["runtime_factor"], 0.005)
        self.assertAlmostEqual(nyra["efficiency"], 0.002)
        # Rust's 0.3 ms is below the floor: it counts as 1 ms, so noise cannot make it look infinitely fast
        self.assertAlmostEqual(rust["runtime_factor"], report.RUNTIME_FLOOR_MS / 1000.0)
        self.assertAlmostEqual(rust["efficiency"], 1.2 * 0.001)

    def test_per_language_runtime_is_over_speed_tasks_only(self):
        stats = self.summary()["langs"]
        self.assertEqual((stats["nyra"]["median_runtime_ms_speed"], stats["nyra"]["runtime_runs_speed"]), (5.0, 3))
        self.assertEqual(stats["python"]["median_code_tokens_first_attempt"], 500)
        self.assertIsNone(report.summarize(speed_records(self.table(), self.LANGS), self.LANGS)["langs"]["nyra"]
                          ["median_runtime_ms_speed"])  # no categories: nothing is known to be a speed task

    def test_without_python_the_first_language_is_the_reference(self):
        langs = ["nyra", "rust"]
        eff = report.summarize(speed_records(self.table(), langs), langs, self.CATS)["efficiency"]
        self.assertEqual(eff["reference"], "nyra")
        self.assertEqual(eff["langs"]["nyra"]["efficiency"], 1.0)

    def test_the_model_report_shows_the_medians_and_every_speed_task(self):
        results = fake_results("vendor/m", speed_records(self.table(), self.LANGS), self.LANGS, self.CATS,
                               timing={"runs": 3, "startup_ms": {}})
        md = report.render_markdown(results, {})
        for needle in ("## Tokens and runtime (medians)", "## Runtime per speed task (ms)",
                       "**Runtime, speed tasks** (median ms, passed runs)", "| s2 | 5.00 | 1,000 | 60.0 | 0.30 |",
                       "**0.002**", "median of 3 timed runs"):
            self.assertIn(needle, md)
        self.assertTrue(md.isascii())

    def test_the_comparison_has_a_runtime_and_an_efficiency_table_only_when_measured(self):
        timed = fake_results("vendor/m", speed_records(self.table(), self.LANGS), self.LANGS, self.CATS)
        md = report.render_comparison([timed])
        self.assertIn("## Runtime on `speed` tasks (median ms)", md)
        self.assertIn("| vendor/m | 0.002 | 1.00 | 0.048 | 0.0012 |", md)
        untimed = speed_records(self.table(), self.LANGS)
        for r in untimed:
            r["attempts"][0]["result"]["runtime_ms"] = None
        md = report.render_comparison([fake_results("vendor/m", untimed, self.LANGS, self.CATS)])
        self.assertNotIn("Runtime on", md)
        self.assertNotIn("Efficiency", md)

    def test_formatting(self):
        self.assertEqual([report.ms(v) for v in (None, 1234.4, 56.78, 8.904)], ["n/a", "1,234", "56.8", "8.90"])
        self.assertEqual([report.factor_text(v) for v in (None, 1.0, 0.4239, 0.00208)], ["n/a", "1.00", "0.42", "0.0021"])
        self.assertIsNone(report.factor(1.0, 0.0))
        self.assertEqual(report.factor(0.2, 4.0, 1.0), 0.25)


class TokenLimitAndSelfRepairReport(unittest.TestCase):
    def records(self):
        rec = ReportSummary.rec
        hit = rec("a", "nyra", False, True, kind="no_code", attempts=2)
        hit["attempts"][0]["stop_reason"] = "max_tokens"
        hit["attempts"][0]["self_repair"] = {"tried": False}
        fixed = rec("b", "nyra", False, True, kind="compile_error", attempts=2)
        fixed["attempts"][0]["self_repair"] = {"tried": True, "changed": True, "fixed": 1,
                                               "result": {"passed": True, "kind": "pass"}}
        unfixed = rec("c", "nyra", False, True, kind="compile_error", attempts=2)
        unfixed["attempts"][0]["self_repair"] = {"tried": True, "changed": False, "fixed": 0, "result": None}
        ok = rec("d", "nyra", True, True)
        ok["attempts"][0]["self_repair"] = {"tried": False}
        py = [rec(t, "python", True, True) for t in "abcd"]
        py_hit = rec("e", "python", False, False, kind="no_code", attempts=2)
        for a in py_hit["attempts"]:
            a["stop_reason"] = "max_tokens"
        e = rec("e", "nyra", True, True)
        e["attempts"][0]["self_repair"] = {"tried": False}
        return [hit, fixed, unfixed, ok, e] + py + [py_hit]

    def test_statistics(self):
        s = report.summarize(self.records(), ["nyra", "python"])["langs"]
        nyra, py = s["nyra"], s["python"]
        self.assertEqual((nyra["pass_at_1"], nyra["pass_at_1_self_repair"]), (2, 3))
        self.assertEqual((nyra["self_repair_tried"], nyra["self_repair_changed"], nyra["self_repair_passed"]), (2, 1, 1))
        self.assertIsNone(py["pass_at_1_self_repair"])  # no such tool: not measured, not faked
        self.assertEqual((nyra["no_code_max_tokens_first_attempt"], nyra["no_code_max_tokens_attempts"]), (1, 1))
        self.assertEqual((py["no_code_max_tokens_first_attempt"], py["no_code_max_tokens_attempts"]), (1, 2))

    def test_reports_label_both_numbers(self):
        results = fake_results("vendor/m", self.records(), ["nyra", "python"])
        md = report.render_markdown(results, {})
        self.assertIn("| pass@1 with self-repair (`nyra check --fix`, no model call) | 60% (3/5) [23-88%], +1 of 2 "
                      "tried | - |", md)
        self.assertIn("| No program: the reply hit the token limit (first attempts / all attempts) | 1 / 1 | 1 / 2 |", md)
        self.assertIn("| of which the reply hit the token limit | 1 | 1 |", md)
        self.assertIn("**pass@1 with self-repair** (Nyra only)", md)
        cmp_md = report.render_comparison([results])
        self.assertIn("## First-try success with Nyra's self-repair", cmp_md)
        self.assertIn("| vendor/m | 60% (3/5), +1 of 2 tried | - |", cmp_md)
        self.assertIn("## No program because the reply hit the token limit", cmp_md)

    def test_runs_without_either_have_no_such_rows(self):
        recs = [ReportSummary.rec("a", lang, True, True) for lang in ("nyra", "python")]
        md = report.render_markdown(fake_results("m", recs, ["nyra", "python"]), {})
        self.assertNotIn("self-repair (", md)
        self.assertNotIn("token limit (first", md)


class OldResultFiles(unittest.TestCase):
    """Result files written before runtimes were measured (schema 2) must stay readable."""

    def old(self, model):
        recs = four_language_records({(t, lang): True for t in ("a", "b") for lang in LANGS4})
        results = fake_results(model, recs, LANGS4, {"a": "math", "b": "speed"})
        for r in recs:
            for a in r["attempts"]:
                a["result"].pop("runtime_ms", None)
        for stats in results["summary"]["langs"].values():  # the summary as schema 2 wrote it
            for key in [k for k in stats if "median" in k or "runtime" in k or "self_repair" in k or "max_tokens" in k]:
                del stats[key]
        del results["summary"]["efficiency"]
        return results

    def test_an_old_summary_is_recomputed_with_empty_runtime_cells(self):
        results = report.ensure_current_summary(self.old("m"))
        self.assertIn("efficiency", results["summary"])
        self.assertIsNone(results["summary"]["efficiency"]["langs"]["nyra"]["median_runtime_ms"])
        self.assertEqual(results["summary"]["langs"]["nyra"]["pass_at_1"], 2)  # the old numbers stay
        self.assertFalse(report.has_runtimes(results))

    def test_publish_reads_old_and_new_files(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "old.json"
            path.write_text(json.dumps(self.old("vendor/old")), encoding="utf-8")
            md, data, _ = publish.publish([path], "x", Path(tmp) / "pub", write=False)
        self.assertIn("Runtime: not measured in these runs.", md)
        self.assertNotIn("## Runtime on", md)
        self.assertIsNone(data["models"]["vendor/old"]["efficiency"]["langs"]["nyra"]["efficiency"])

    def test_publish_includes_runtime_and_efficiency(self):
        rt = RuntimeReport()
        results = fake_results("vendor/new", speed_records(rt.table(), LANGS4), LANGS4, rt.CATS,
                               timing={"runs": 3, "jobs": 4, "startup_ms": {"python": 20.0, "nyra": 5.0}})
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "new.json"
            path.write_text(json.dumps(results), encoding="utf-8")
            md, data, _ = publish.publish([path], "x", Path(tmp) / "pub", write=False)
        for needle in ("## Runtime on `speed` tasks (median ms)", "## Efficiency: code tokens x runtime",
                       "#### Tokens and runtime (medians)", "every passing program was run 3 more times",
                       "Python 20.0 ms", "**Efficiency** = (median code tokens / Python's)"):
            self.assertIn(needle, md)
        self.assertAlmostEqual(data["models"]["vendor/new"]["efficiency"]["langs"]["nyra"]["efficiency"], 0.002)
        self.assertEqual(data["run"]["timing"]["runs"], 3)
        self.assertTrue(md.isascii())


@needs_nyra
class NyraSelfRepair(unittest.TestCase):
    BROKEN = "fn main() {\n    print(f(2))\n}\nfn f(x: int) -> int {\n    return x + 40\n}\n"  # `return`: E0201

    def test_the_compiler_repairs_an_unambiguous_mistake(self):
        lang = run.NyraLang(NYRA, timeout=10)
        self.assertEqual(lang.evaluate(self.BROKEN, _task("42\n")).kind, "compile_error")
        out = lang.self_repair(self.BROKEN, _task("42\n"))
        self.assertTrue(out["tried"] and out["changed"])
        self.assertTrue(out["result"]["passed"], out)
        self.assertIn("ret x + 40", out["code"])

    def test_a_mistake_without_a_fix_is_left_alone(self):
        out = run.NyraLang(NYRA, timeout=10).self_repair("fn main() {\n    print(nothing)\n}\n", _task("1\n"))
        self.assertEqual((out["changed"], out["result"]), (False, None))
        self.assertIn("E0201", out["remaining"])

    def run_once(self, reply, **ctx):
        class Fixed(providers.Provider):
            name, default_model = "f", "f"

            def complete(self, system, messages, meta):
                text = reply if meta["attempt"] == 1 else "```\nfn main() {\n    print(42)\n}\n```"
                return providers.Reply(text, providers.Usage(1, 1))

        return run.run_one(_task("42\n"), run.NyraLang(NYRA, timeout=10), 0,
                           run.RunContext(provider=Fixed(), repairs=1, count_tokens=False, **ctx))

    def test_the_attempt_loop_records_it_on_the_side(self):
        rec = self.run_once("```\n" + self.BROKEN + "```", self_repair=True)
        first = rec["attempts"][0]
        self.assertEqual(first["result"]["kind"], "compile_error")
        self.assertTrue(first["self_repair"]["result"]["passed"])
        # the model still got its feedback and its repair attempt: plain pass@1 is unchanged
        self.assertEqual((rec["first_try"], rec["attempts_used"], rec["status"]), (False, 2, "pass"))
        self.assertNotIn("self_repair", rec["attempts"][1])
        good = self.run_once("```\nfn main() {\n    print(42)\n}\n```", self_repair=True)
        self.assertEqual(good["attempts"][0]["self_repair"], {"tried": False})
        off = self.run_once("```\n" + self.BROKEN + "```")
        self.assertNotIn("self_repair", off["attempts"][0])


@needs_nyra
class MockPipelineTiming(unittest.TestCase):
    def run_main_out(self, *extra):
        with tempfile.TemporaryDirectory() as out:
            code, stdout, stderr = _run_main("--provider", "mock", "--langs", "nyra,python", "--tasks",
                                             "fizzbuzz,matrix_mult", "--out", out, "-q", *extra)
            results = json.loads(next(Path(out).glob("*.json")).read_text(encoding="utf-8"))
        return code, stdout, stderr, results

    def test_every_passing_program_is_timed_and_the_method_is_recorded(self):
        code, stdout, stderr, results = self.run_main_out("--time-runs", "2")
        self.assertEqual(code, 0, stderr)
        self.assertEqual(results["schema"], 3)
        timing = results["run"]["timing"]
        self.assertEqual((timing["runs"], timing["statistic"]), (2, "median"))
        self.assertTrue(all(timing["startup_ms"][lang] > 0 for lang in ("nyra", "python")))
        for rec in results["records"]:
            res = rec["attempts"][0]["result"]
            self.assertEqual(len(res["timing"]["runs_ms"]), 2)
            self.assertGreaterEqual(res["runtime_ms"], 0)
            self.assertEqual(rec["attempts"][0]["self_repair"] if rec["lang"] == "nyra" else None,
                             {"tried": False} if rec["lang"] == "nyra" else None)
        self.assertTrue(results["run"]["self_repair"])
        stats = results["summary"]["langs"]
        self.assertEqual((stats["nyra"]["runtime_runs_speed"], stats["python"]["runtime_runs_speed"]), (1, 1))
        self.assertIn("## Tokens and runtime (medians)", stdout)
        self.assertIn("| matrix_mult |", stdout)

    def test_timing_and_self_repair_can_be_switched_off(self):
        code, _, stderr, results = self.run_main_out("--time-runs", "0", "--no-self-repair")
        self.assertEqual(code, 0, stderr)
        self.assertEqual(results["run"]["timing"]["runs"], 0)
        self.assertFalse(results["run"]["self_repair"])
        for rec in results["records"]:
            self.assertIsNone(rec["attempts"][0]["result"]["runtime_ms"])
            self.assertNotIn("self_repair", rec["attempts"][0])
        code, _, stderr = _run_main("--provider", "mock", "--time-runs", "-1", "--dry-run")
        self.assertEqual(code, 2)
        self.assertIn("--time-runs", stderr)


class SpeedTool(unittest.TestCase):
    """bench/speed.py: the reference solutions timed alone, no model."""

    def setUp(self):
        import speed
        self.speed = speed

    def main(self, *argv):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = self.speed.main(list(argv))
        return code, out.getvalue(), err.getvalue()

    def test_the_default_is_the_speed_tasks(self):
        tasks = run.load_tasks()
        chosen = self.speed.select_tasks(tasks, None, False)
        self.assertEqual({t.category for t in chosen}, {report.SPEED_CATEGORY})
        self.assertEqual(len(self.speed.select_tasks(tasks, None, True)), len(tasks))
        self.assertEqual([t.id for t in self.speed.select_tasks(tasks, "fizz*", False)], ["fizzbuzz"])
        with self.assertRaises(run.UsageError):
            self.speed.select_tasks(tasks, "no_such_task", False)

    def test_a_python_only_run_prints_the_table_and_writes_files(self):
        with tempfile.TemporaryDirectory() as tmp:
            code, out, err = self.main("--tasks", "fizzbuzz,gcd_pairs", "--langs", "python", "--runs", "2",
                                       "--out", tmp, "-q")
            files = sorted(p.name for p in Path(tmp).iterdir())
            data = json.loads(next(Path(tmp).glob("*.json")).read_text(encoding="utf-8"))
        self.assertEqual(code, 0, err)
        self.assertEqual([f.rsplit("-", 2)[-2:] for f in files], [["speed", "references.json"],
                                                                  ["speed", "references.md"]])
        self.assertIn("## Runtime (ms)", out)
        self.assertIn("| **Relative to Python** | **1.00** |", out)
        self.assertEqual(data["kind"], "speed-references")
        self.assertEqual(data["summary"]["tasks"], ["fizzbuzz", "gcd_pairs"])
        cell = data["results"]["fizzbuzz"]["python"]
        self.assertTrue(cell["passed"])
        self.assertEqual(len(cell["timing"]["runs_ms"]), 2)
        self.assertIsNotNone(data["startup_ms"]["python"])

    def test_summary_and_rendering(self):
        results = {"t1": {"nyra": {"passed": True, "kind": "pass", "runtime_ms": 10.0, "compile_ms": 500.0},
                          "python": {"passed": True, "kind": "pass", "runtime_ms": 1000.0, "compile_ms": None}},
                   "t2": {"nyra": {"passed": False, "kind": "wrong_output"},
                          "python": {"passed": True, "kind": "pass", "runtime_ms": 3000.0, "compile_ms": None}}}
        summary = self.speed.summarize(results, ["nyra", "python"])
        self.assertEqual(summary["tasks"], ["t1"])  # only the tasks every language ran
        self.assertAlmostEqual(summary["langs"]["nyra"]["runtime_factor"], 0.01)
        data = {"langs": ["nyra", "python"], "summary": summary, "machine": {"platform": "p", "python": "3", "nyra": "n"},
                "backend": "native", "date": "d", "tasks": ["t1", "t2"], "runs": 5, "results": results,
                "startup_ms": {"nyra": 1.0, "python": 20.0}}
        md = self.speed.render(data)
        self.assertIn("| t2 | FAIL (wrong_output) | 3,000 |", md)
        self.assertIn("| **Relative to Python** | **0.01** | **1.00** |", md)
        self.assertIn("| Compile (median over the tasks) | 500 | - |", md)

    def test_bad_arguments(self):
        self.assertEqual(self.main("--runs", "0", "--langs", "python")[0], 2)
        self.assertEqual(self.main("--langs", "python,python")[0], 2)

    @unittest.skipUnless(NYRA and NODE, "needs the nyra compiler and Node.js")
    def test_nyra_runs_as_a_native_executable_and_on_the_js_backend(self):
        for backend in ("native", "js"):
            code, out, err = self.main("--tasks", "fizzbuzz", "--langs", "nyra,python", "--runs", "1", "--no-files",
                                       "-q", "--backend", backend)
            self.assertEqual(code, 0, err)
            self.assertIn(f"({backend})", out)


class ContinuousIntegration(unittest.TestCase):
    def test_ci_runs_the_benchmark_tests_and_the_verifier(self):
        ci = (REPO_ROOT / ".github" / "workflows" / "ci.yml").read_text(encoding="utf-8")
        for needle in ("cargo test", "cargo build --release", "python bench/test_bench.py", "python bench/verify.py",
                       "ubuntu-latest", "actions/setup-node", "actions/setup-python"):
            self.assertIn(needle, ci)


class Readme(unittest.TestCase):
    def test_the_readme_documents_every_option_and_tool(self):
        readme = (BENCH_DIR / "README.md").read_text(encoding="utf-8")
        options = {s for action in run.build_parser()._actions for s in action.option_strings if s.startswith("--")}
        options.discard("--help")
        self.assertGreater(len(options), 20)
        for option in sorted(options):
            self.assertIn(option, readme, f"bench/README.md does not mention {option}")
        for needle in ("OPENROUTER_API_KEY", "bench/models.py", "bench/models.json", "bench/publish.py",
                       "bench/published", "typescript", "rust", "ANTHROPIC_API_KEY"):
            self.assertIn(needle, readme)

    def test_the_readme_only_names_models_that_bench_models_json_ships(self):
        # the ids the README tells the reader to type must be ids the harness itself ships and verified
        readme = (BENCH_DIR / "README.md").read_text(encoding="utf-8")
        shipped = set(modelsmod.default_model_ids())
        for mid in set(re.findall(r"\b(?:anthropic|openai|google|x-ai|deepseek)/[A-Za-z0-9._-]+", readme)):
            self.assertIn(mid, shipped, f"README mentions {mid}, which is not in bench/models.json")


if __name__ == "__main__":
    unittest.main(verbosity=2)
