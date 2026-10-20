import os
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import _setup  # noqa: F401  (sys.path and NYRA_PATH)
from _setup import HAVE_BINARY, SKIP_REASON

import nyra
from nyra import NyraError, Sandbox

LOOP = "var i = 0\nwhile true {\n    i += 1\n}\n"
FIXABLE = "let xs = [1, 2]\nxs.append(3)\nprint(len(xs));\n"


@unittest.skipUnless(HAVE_BINARY, SKIP_REASON)
class RunTest(unittest.TestCase):
    def setUp(self):
        self.box = Sandbox(fuel=5_000_000, timeout=10)

    def test_prints_and_reads_stdin(self):
        r = self.box.run('use input\nlet name = input.line()\nprint("hi {name}")\n', stdin="Ada\n")
        self.assertTrue(r.ok, r.stderr)
        self.assertEqual(r.stdout, "hi Ada\n")
        self.assertEqual(r.exit_code, 0)
        self.assertEqual(r.errors, [])
        self.assertIsNone(r.stage)
        self.assertIsInstance(r.steps, int)
        self.assertGreater(r.steps, 0)
        self.assertGreater(r.ms, 0)
        self.assertEqual(r.backend, "native")

    def test_unicode_round_trips(self):
        r = self.box.run('use input\nprint(input.all())\n', stdin="žluťoučký kůň 🐎")
        self.assertEqual(r.stdout, "žluťoučký kůň 🐎\n")

    def test_compile_error_is_structured(self):
        r = self.box.run("let x = 1\nprint(y)\n")
        self.assertFalse(r.ok)
        self.assertEqual(r.stage, "compile")
        self.assertEqual(r.exit_code, 1)
        e = r.errors[0]
        self.assertEqual((e["code"], e["line"], e["col"]), ("E0201", 2, 7))
        self.assertIn("y", e["message"])
        self.assertTrue(e["hint"])
        self.assertIn("fix", e)
        self.assertEqual(e["file"], "main.nyra")
        self.assertIn("E0201", r.stderr)
        self.assertEqual(r.stdout, "")

    def test_runtime_error_is_structured(self):
        r = self.box.run("let xs = [1, 2]\nprint(xs[5])\n")
        self.assertFalse(r.ok)
        self.assertEqual(r.stage, "runtime")
        self.assertEqual(r.exit_code, 101)
        self.assertEqual(r.errors[0]["code"], "E0240")
        self.assertEqual(r.errors[0]["line"], 2)
        self.assertTrue(r.errors[0]["runtime"])
        self.assertIsNone(r.limit)

    def test_fuel_limit(self):
        r = Sandbox(fuel=100_000).run(LOOP)
        self.assertEqual((r.exit_code, r.limit), (120, "fuel"))
        self.assertEqual(r.errors[0]["code"], "E0355")
        self.assertGreaterEqual(r.steps, 100_000)

    def test_time_limit(self):
        r = Sandbox(fuel=10**15, timeout=0.3).run(LOOP)
        self.assertEqual((r.exit_code, r.limit), (124, "time"))
        self.assertEqual(r.errors[0]["code"], "E0359")
        self.assertLess(r.ms, 5000)

    def test_memory_limit(self):
        r = Sandbox(memory_mb=8).run('var s = "x"\nwhile true {\n    s = s + s\n}\n')
        self.assertEqual((r.exit_code, r.limit), (121, "memory"), r.stderr)

    def test_output_limit(self):
        r = Sandbox(max_output=100).run('for i in 0..1000 {\n    print("line {i}")\n}\n')
        self.assertEqual((r.exit_code, r.limit), (122, "output"), r.stderr)
        self.assertLessEqual(len(r.stdout.encode()), 100)

    def test_depth_limit(self):
        r = Sandbox(max_depth=50).run("fn f(n: int) -> int = f(n + 1)\nprint(f(0))\n")
        self.assertEqual((r.exit_code, r.limit), (123, "depth"), r.stderr)

    def test_no_capability_by_default(self):
        for mod in ("fs", "os"):
            r = self.box.run(f"use {mod}\nprint(1)\n")
            self.assertEqual(r.stage, "compile")
            self.assertEqual(r.errors[0]["code"], "E0290")

    def test_fs_is_confined_to_cwd(self):
        with tempfile.TemporaryDirectory() as d:
            box = Sandbox(allow=["fs"], cwd=d)
            r = box.run('use fs\nfs.write("note.txt", "kept")\nprint(fs.read("note.txt"))\n')
            self.assertTrue(r.ok, r.stderr)
            self.assertEqual(r.stdout, "kept\n")
            self.assertEqual(Path(d, "note.txt").read_text(), "kept")
            r = box.run('use fs\nfs.write("../escaped.txt", "no")\n')
            self.assertFalse(r.ok)
            self.assertFalse(Path(d).parent.joinpath("escaped.txt").exists())

    def test_args_with_os(self):
        r = Sandbox(allow="os").run("use os\nprint(os.args())\n", args=["a", "b c"])
        self.assertEqual(r.stdout, '["a", "b c"]\n', r.stderr)

    def test_modules_next_to_main(self):
        files = {"shapes.nyra": "pub struct Rect { w: int, h: int }\npub fn area(r: Rect) -> int = r.w * r.h\n"}
        r = self.box.run("use ./shapes\nprint(shapes.area(Rect(w: 3, h: 4)))\n", files=files)
        self.assertEqual(r.stdout, "12\n", r.stderr)
        with self.assertRaises(ValueError):
            self.box.run("print(1)\n", files={"../evil.nyra": ""})

    def test_certain_fixes_in_memory_unless_strict(self):
        r = self.box.run(FIXABLE)
        self.assertTrue(r.ok, r.stderr)
        self.assertEqual(r.stdout, "3\n")
        self.assertTrue(any(w.get("applied") for w in r.warnings))
        self.assertIn("repaired in memory", r.feedback())
        r = Sandbox(strict=True).run(FIXABLE)
        self.assertEqual(r.stage, "compile")

    def test_feedback_names_the_error(self):
        text = self.box.run("let xs = [1, 2]\nprint(xs[5])\n").feedback()
        self.assertIn("E0240", text)
        self.assertIn("line 2", text)

    def test_always_sandboxed_and_temp_dirs_removed(self):
        made = []
        real = tempfile.TemporaryDirectory

        def spy(*a, **k):
            t = real(*a, **k)
            made.append(t.name)
            return t

        calls = []
        real_exec = Sandbox._exec

        def exec_spy(self, args, *rest):
            calls.append(args)
            return real_exec(self, args, *rest)

        with mock.patch("tempfile.TemporaryDirectory", spy), mock.patch.object(Sandbox, "_exec", exec_spy):
            self.box.run('print("x")\n')
            self.box.check('print("x")\n')
            self.box.fix('print("x")\n')
        self.assertTrue(made)
        self.assertFalse([d for d in made if os.path.exists(d)])
        run = calls[0]
        self.assertIn("--sandbox", run)
        self.assertNotIn("--native", run)

    def test_bad_options(self):
        with self.assertRaises(ValueError):
            Sandbox(allow=["root"])
        with self.assertRaises(ValueError):
            Sandbox(fuel=0)
        with self.assertRaises(nyra.NyraNotFound):
            Sandbox(binary=os.path.join(tempfile.gettempdir(), "no-such-nyra"))


@unittest.skipUnless(HAVE_BINARY, SKIP_REASON)
class ToolsOfTheCompilerTest(unittest.TestCase):
    def setUp(self):
        self.box = Sandbox()

    def test_check(self):
        c = self.box.check('print("ok")\n')
        self.assertTrue(c.ok)
        c = self.box.check("let x = 1\nprint(y)\n")
        self.assertFalse(c.ok)
        self.assertEqual(c.errors[0]["code"], "E0201")
        # strict by default: a fixable error is an error
        c = self.box.check(FIXABLE)
        self.assertFalse(c.ok)
        self.assertTrue(c.errors[0]["fix"])
        c = self.box.check(FIXABLE, strict=False)
        self.assertTrue(c.ok)
        self.assertTrue(c.warnings)

    def test_check_runs_examples(self):
        c = self.box.check("fn double(x: int) -> int = x * 3\nex double(2) == 4\nprint(double(1))\n")
        self.assertFalse(c.ok)

    def test_fix(self):
        self.assertEqual(self.box.fix(FIXABLE), "var xs = [1, 2]\nxs.push(3)\nprint(xs.len())\n")
        broken = "let xs = [1]\nxs.append(2)\nprint(cout)\n"
        self.assertEqual(self.box.fix(broken), broken)

    def test_explain(self):
        e = self.box.explain("E0201")
        self.assertEqual(e["code"], "E0201")
        self.assertTrue(e["ok"])
        self.assertIn("E0201", e["text"])
        self.assertFalse(self.box.explain("E9999")["ok"])

    def test_run_json(self):
        v = self.box.run_json('use json\nstruct P { x: int, tags: [str] }\nprint(json.str(P(x: 1, tags: ["a"])))\n')
        self.assertEqual(v, {"x": 1, "tags": ["a"]})
        self.assertEqual(self.box.run_json('print("log line")\nprint("[1, 2]")\n'), [1, 2])
        with self.assertRaises(NyraError) as ctx:
            self.box.run_json("print(nope)\n")
        self.assertEqual(ctx.exception.result.errors[0]["code"], "E0201")
        with self.assertRaises(NyraError):
            self.box.run_json('print("not json")\n')

    def test_module_functions_and_version(self):
        self.assertEqual(nyra.run('print(1 + 2)\n').stdout, "3\n")
        self.assertTrue(nyra.check('print(1)\n').ok)
        self.assertRegex(self.box.version(), r"^\d+\.\d+\.\d+")


if __name__ == "__main__":
    unittest.main()
