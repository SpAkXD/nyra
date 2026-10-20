"""Nyra as a tool for agent frameworks, without importing any of them up front.

Each helper returns the framework's own tool object when that framework is installed (imported
lazily, at the call), and otherwise a `NyraTool`: a plain callable with `name`, `description` and a
JSON schema in `parameters`. The tool runs code in a `Sandbox` and returns `Result.feedback()`, a
short text with the output and each error's code, line and hint, so the model can fix and retry.

    from nyra.tools import anthropic_tool_spec, run_tool
    tools = [anthropic_tool_spec()]            # for client.messages.create(tools=...)
    text = run_tool(block.input)               # when the model calls it
"""

from __future__ import annotations

from typing import Any, Callable, Dict, Mapping, Optional, Type

from ._sandbox import Sandbox

TOOL_NAME = "run_nyra"
TOOL_DESCRIPTION = (
    "Run a program written in Nyra in a sandbox and get back its standard output, or its errors. "
    "Nyra is a small statically typed language: `let` (immutable) and `var`, `fn name(a: int) -> int { ... }`, "
    "`struct`, `enum`, `match`, `for x in xs`, `for i in 0..n`, arrays `[int]`, maps `[str: int]`, "
    "string interpolation \"{expr}\". Print results with `print(...)`; `use json` and `print(json.str(value))` "
    "prints a value as JSON. Read the `stdin` argument with `use input` and `input.line()` / `input.all()`. "
    "No files, network or OS access. Every error comes back with a code (E0xxx), a line and a hint: fix the "
    "code and call the tool again."
)
PARAMETERS: Dict[str, Any] = {
    "type": "object",
    "properties": {
        "code": {"type": "string", "description": "The complete Nyra program."},
        "stdin": {"type": "string", "description": "Standard input for the program (optional)."},
    },
    "required": ["code"],
    "additionalProperties": False,
}


def _runner(sandbox: Optional[Sandbox]) -> Callable[..., str]:
    box = sandbox
    def run_nyra(code: str, stdin: str = "") -> str:
        """Run a Nyra program in a sandbox and return its output or its errors."""
        nonlocal box
        if box is None:
            box = Sandbox()
        return box.run(code, stdin or "").feedback()

    return run_nyra


class NyraTool:
    """A plain tool: call it with `code` (and `stdin`), or `.invoke({"code": ...})`."""

    def __init__(self, func: Callable[..., str], name: str = TOOL_NAME, description: str = TOOL_DESCRIPTION):
        self.func = func
        self.name = name
        self.description = description
        self.parameters = PARAMETERS

    def __call__(self, code: str, stdin: str = "") -> str:
        return self.func(code, stdin)

    def invoke(self, tool_input: Mapping[str, Any]) -> str:
        return self.func(tool_input["code"], tool_input.get("stdin") or "")

    def __repr__(self) -> str:
        return f"NyraTool(name={self.name!r})"


def run_tool(tool_input: Mapping[str, Any], sandbox: Optional[Sandbox] = None) -> str:
    """Runs the input of a `run_nyra` tool call ({"code": ..., "stdin": ...}) and returns the text for the result."""
    return _runner(sandbox)(tool_input["code"], tool_input.get("stdin") or "")


# ---- raw API tool specs ---------------------------------------------------------------------------


def anthropic_tool_spec(name: str = TOOL_NAME, description: str = TOOL_DESCRIPTION) -> Dict[str, Any]:
    """A tool for the Anthropic Messages API (`tools=[...]`)."""
    return {"name": name, "description": description, "input_schema": dict(PARAMETERS)}


def openai_tool_spec(name: str = TOOL_NAME, description: str = TOOL_DESCRIPTION) -> Dict[str, Any]:
    """A function tool for OpenAI-style chat completions (`tools=[...]`)."""
    return {"type": "function", "function": {"name": name, "description": description, "parameters": dict(PARAMETERS)}}


# ---- frameworks (lazy) -----------------------------------------------------------------------------


def langchain_tool(sandbox: Optional[Sandbox] = None) -> Any:
    """A LangChain `StructuredTool` (langchain-core), or a `NyraTool` without LangChain."""
    fn = _runner(sandbox)
    try:
        from langchain_core.tools import StructuredTool
    except ImportError:
        return NyraTool(fn)
    return StructuredTool.from_function(func=fn, name=TOOL_NAME, description=TOOL_DESCRIPTION)


def llamaindex_tool(sandbox: Optional[Sandbox] = None) -> Any:
    """A LlamaIndex `FunctionTool`, or a `NyraTool` without LlamaIndex."""
    fn = _runner(sandbox)
    try:
        from llama_index.core.tools import FunctionTool
    except ImportError:
        return NyraTool(fn)
    return FunctionTool.from_defaults(fn=fn, name=TOOL_NAME, description=TOOL_DESCRIPTION)


def crewai_tool(sandbox: Optional[Sandbox] = None) -> Any:
    """A CrewAI `BaseTool`, or a `NyraTool` without CrewAI."""
    fn = _runner(sandbox)
    try:
        from crewai.tools import BaseTool
        from pydantic import BaseModel, Field
    except ImportError:
        return NyraTool(fn)

    class RunNyraInput(BaseModel):
        code: str = Field(..., description=PARAMETERS["properties"]["code"]["description"])
        stdin: str = Field("", description=PARAMETERS["properties"]["stdin"]["description"])

    class RunNyraTool(BaseTool):
        name: str = TOOL_NAME
        description: str = TOOL_DESCRIPTION
        args_schema: Type[BaseModel] = RunNyraInput

        def _run(self, code: str, stdin: str = "") -> str:
            return fn(code, stdin)

    return RunNyraTool()


def autogen_function(sandbox: Optional[Sandbox] = None) -> Any:
    """An AutoGen `FunctionTool` (autogen-core 0.4+), or a `NyraTool` without it (for AutoGen 0.2,
    pass `tool.func` to `register_function`)."""
    fn = _runner(sandbox)
    try:
        from autogen_core.tools import FunctionTool
    except ImportError:
        return NyraTool(fn)
    return FunctionTool(fn, description=TOOL_DESCRIPTION, name=TOOL_NAME)


__all__ = [
    "TOOL_NAME",
    "TOOL_DESCRIPTION",
    "PARAMETERS",
    "NyraTool",
    "run_tool",
    "anthropic_tool_spec",
    "openai_tool_spec",
    "langchain_tool",
    "llamaindex_tool",
    "crewai_tool",
    "autogen_function",
]
