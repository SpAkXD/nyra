import importlib.util
import json
import sys
import types
import unittest
from unittest import mock

import _setup  # noqa: F401
from _setup import HAVE_BINARY, SKIP_REASON

from nyra import tools


def installed(name):
    try:
        return importlib.util.find_spec(name) is not None
    except (ImportError, ValueError):
        return False


def fake_module(path, **attrs):
    """sys.modules entries for `path` and its parents, the last one holding `attrs`."""
    mods = {}
    parts = path.split(".")
    for i in range(1, len(parts) + 1):
        mods[".".join(parts[:i])] = types.ModuleType(".".join(parts[:i]))
    for k, v in attrs.items():
        setattr(mods[path], k, v)
    return mods


class SpecTest(unittest.TestCase):
    def test_anthropic_spec(self):
        s = tools.anthropic_tool_spec()
        self.assertEqual(s["name"], "run_nyra")
        self.assertEqual(s["input_schema"]["required"], ["code"])
        self.assertIn("stdin", s["input_schema"]["properties"])
        json.dumps(s)

    def test_openai_spec(self):
        s = tools.openai_tool_spec()
        self.assertEqual(s["type"], "function")
        self.assertEqual(s["function"]["name"], "run_nyra")
        self.assertEqual(s["function"]["parameters"]["type"], "object")
        json.dumps(s)

    def test_specs_do_not_share_the_schema(self):
        tools.anthropic_tool_spec()["input_schema"]["x"] = 1
        self.assertNotIn("x", tools.PARAMETERS)

    def test_no_framework_is_imported(self):
        for name in ("langchain_core", "llama_index", "crewai", "autogen_core"):
            if not installed(name):
                self.assertNotIn(name, sys.modules)


@unittest.skipUnless(HAVE_BINARY, SKIP_REASON)
class FallbackTest(unittest.TestCase):
    def test_plain_callable_without_frameworks(self):
        for make, mod in (
            (tools.langchain_tool, "langchain_core"),
            (tools.llamaindex_tool, "llama_index"),
            (tools.crewai_tool, "crewai"),
            (tools.autogen_function, "autogen_core"),
        ):
            if installed(mod):
                continue
            t = make()
            self.assertIsInstance(t, tools.NyraTool)
            self.assertEqual(t.name, "run_nyra")
            self.assertEqual(t.parameters["required"], ["code"])
            self.assertIn("3", t('print(1 + 2)\n'))
            self.assertIn("E0201", t.invoke({"code": "print(nope)\n"}))

    def test_run_tool(self):
        out = tools.run_tool({"code": "use input\nprint(input.line())\n", "stdin": "echo\n"})
        self.assertIn("echo", out)


@unittest.skipUnless(HAVE_BINARY, SKIP_REASON)
class AdapterTest(unittest.TestCase):
    """The adapters against stand-ins of the frameworks' classes (the real ones are not installed here)."""

    def test_langchain(self):
        seen = {}

        class StructuredTool:
            @classmethod
            def from_function(cls, **kw):
                seen.update(kw)
                return "lc-tool"

        with mock.patch.dict(sys.modules, fake_module("langchain_core.tools", StructuredTool=StructuredTool)):
            self.assertEqual(tools.langchain_tool(), "lc-tool")
        self.assertEqual(seen["name"], "run_nyra")
        self.assertIn("4", seen["func"]("print(2 + 2)\n"))

    def test_llamaindex(self):
        seen = {}

        class FunctionTool:
            @classmethod
            def from_defaults(cls, **kw):
                seen.update(kw)
                return "li-tool"

        with mock.patch.dict(sys.modules, fake_module("llama_index.core.tools", FunctionTool=FunctionTool)):
            self.assertEqual(tools.llamaindex_tool(), "li-tool")
        self.assertEqual(seen["name"], "run_nyra")
        self.assertIn("5", seen["fn"]("print(2 + 3)\n"))

    def test_autogen(self):
        class FunctionTool:
            def __init__(self, fn, description, name):
                self.fn, self.description, self.name = fn, description, name

        with mock.patch.dict(sys.modules, fake_module("autogen_core.tools", FunctionTool=FunctionTool)):
            t = tools.autogen_function()
        self.assertEqual(t.name, "run_nyra")
        self.assertIn("6", t.fn("print(3 + 3)\n"))

    @unittest.skipUnless(installed("pydantic"), "pydantic is not installed")
    def test_crewai(self):
        from pydantic import BaseModel

        class BaseTool(BaseModel):
            name: str
            description: str

            def run(self, **kw):
                return self._run(**kw)

        with mock.patch.dict(sys.modules, fake_module("crewai.tools", BaseTool=BaseTool)):
            t = tools.crewai_tool()
        self.assertEqual(t.name, "run_nyra")
        self.assertIn("code", t.args_schema.model_json_schema()["properties"])
        self.assertIn("7", t.run(code="print(3 + 4)\n"))


if __name__ == "__main__":
    unittest.main()
