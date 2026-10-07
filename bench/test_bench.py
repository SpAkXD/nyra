"""Tests for the benchmark harness itself (not for Nyra).

    python bench/test_bench.py             # or: python -m unittest discover -s bench

Standard library only. Tests that need the Nyra compiler are skipped when it is not built;
the SDK test is skipped when the `anthropic` package is not importable. No test calls a real API.
"""

from __future__ import annotations

import contextlib
import http.server
import io
import json
import os
import sys
import tempfile
import threading
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

BENCH_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(BENCH_DIR))

import providers  # noqa: E402
import report  # noqa: E402
import run  # noqa: E402


def _nyra_or_none():
    try:
        return run.find_nyra()
    except run.HarnessError:
        return None


NYRA = _nyra_or_none()
needs_nyra = unittest.skipUnless(NYRA, "the nyra compiler is not built (cargo build --release)")


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
        self.assertTrue(30 <= len(self.tasks) <= 40, len(self.tasks))
        self.assertEqual({t.min_version for t in self.tasks}, {"0.1", "0.2", "0.3"})

    def test_every_task_has_a_python_reference_and_v01_tasks_a_nyra_one(self):
        for t in self.tasks:
            self.assertTrue((run.SOLUTIONS_DIR / "python" / f"{t.id}.py").is_file(), t.id)
            nyra = run.SOLUTIONS_DIR / "nyra" / f"{t.id}.nyra"
            if t.min_version == "0.1":
                self.assertTrue(nyra.is_file(), f"{t.id} needs a Nyra reference")

    def test_no_orphan_solutions(self):
        ids = {t.id for t in self.tasks}
        for lang, ext in (("python", ".py"), ("nyra", ".nyra")):
            for path in (run.SOLUTIONS_DIR / lang).glob(f"*{ext}"):
                self.assertIn(path.stem, ids, f"{path} has no task")

    def test_expected_outputs_are_clean(self):
        for t in self.tasks:
            self.assertTrue(t.expected_output.endswith("\n"), t.id)
            self.assertNotIn("\r", t.expected_output, t.id)
            self.assertEqual(run.normalize_output(t.expected_output) + "\n", t.expected_output, t.id)
            self.assertTrue(t.prompt.strip() and t.title.strip() and t.category, t.id)

    def test_prompts_never_mention_a_language(self):
        for t in self.tasks:
            low = t.prompt.lower()
            self.assertNotIn("python", low, t.id)
            self.assertNotIn("nyra", low, t.id)

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
        with tempfile.TemporaryDirectory() as out:
            code, stdout, stderr = _run_main("--provider", "mock", "--tasks", self.TASKS, "--out", out, "-q", *extra)
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
            code, stdout, _ = _run_main("--provider", "mock", "--tasks", "fizzbuzz", "--out", out, "--dry-run")
            self.assertEqual(code, 0)
            self.assertEqual(list(Path(out).iterdir()), [])
            self.assertIn("at most 8 model calls", stdout)

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
        ft = s["paired"]["first_try"]
        self.assertEqual((ft["both"], ft["only_nyra"], ft["only_python"], ft["neither"]), (1, 0, 1, 1))

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


if __name__ == "__main__":
    unittest.main(verbosity=2)
