"""Nyra as a LangChain tool: an agent that answers by running sandboxed programs.

    pip install nyra langchain langchain-anthropic
    export ANTHROPIC_API_KEY=...
    python sdk/examples/python/langchain_tool.py

`nyra.tools.langchain_tool()` returns a `StructuredTool` named `run_nyra` when langchain-core is
installed (and a plain callable with a JSON schema when it is not). The same goes for
`llamaindex_tool()`, `crewai_tool()` and `autogen_function()`.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "python" / "src"))  # not needed once installed

from nyra import Sandbox
from nyra.tools import langchain_tool


def main() -> None:
    tool = langchain_tool(Sandbox(fuel=50_000_000, timeout=10))
    print(f"tool: {tool.name} ({type(tool).__name__})")
    # the tool on its own, without a model
    print(tool.invoke({"code": "print([x * x for x in 0..5])\n"}))

    try:
        from langchain.agents import create_agent
        from langchain_anthropic import ChatAnthropic
    except ImportError:
        sys.exit("for the agent part: pip install langchain langchain-anthropic")

    agent = create_agent(
        ChatAnthropic(model="claude-opus-5-5"),
        tools=[tool],
        system_prompt="Answer by writing and running Nyra programs with run_nyra; fix them when they fail.",
    )
    out = agent.invoke({"messages": [{"role": "user", "content": "Which 3-digit numbers equal the sum of the cubes of their digits?"}]})
    print(out["messages"][-1].content)


if __name__ == "__main__":
    main()
