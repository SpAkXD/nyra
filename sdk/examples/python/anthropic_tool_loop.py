"""Claude writes Nyra, the sandbox runs it, the errors go back until it works: a manual tool-use loop.

    pip install anthropic nyra
    export ANTHROPIC_API_KEY=...            # or `ant auth login`
    python sdk/examples/python/anthropic_tool_loop.py "How many primes are there below 100000?"

The model gets one tool, `run_nyra`. Each call runs in a Sandbox (no files, OS or network; limits on
steps, memory, output and time), and the result it sees is `Result.feedback()`: the output, or each
error with its code, line and hint. This file only imports `anthropic` when it runs.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "python" / "src"))  # not needed once installed

from nyra import Sandbox
from nyra.tools import TOOL_NAME, anthropic_tool_spec

MODEL = "claude-opus-5-5"
SYSTEM = (
    "You answer questions by writing and running Nyra programs with the run_nyra tool. "
    "If the program fails, read the error codes and hints, fix the program and run it again. "
    "Give the final answer in one sentence."
)


def main(question: str) -> None:
    try:
        import anthropic
    except ImportError:
        sys.exit("this example needs the anthropic package: pip install anthropic")

    client = anthropic.Anthropic()
    box = Sandbox(fuel=50_000_000, timeout=10, memory_mb=128)
    tools = [anthropic_tool_spec()]
    messages = [{"role": "user", "content": question}]

    for _ in range(10):  # at most 10 model turns
        # server-side fallbacks: a request the safety classifiers decline is re-run on another model
        response = client.beta.messages.create(
            model=MODEL,
            max_tokens=16000,
            system=SYSTEM,
            tools=tools,
            messages=messages,
            betas=["server-side-fallback-2026-07-01"],
            fallbacks="default",
        )
        if response.stop_reason == "refusal":
            print("The request was declined.")
            return
        messages.append({"role": "assistant", "content": response.content})
        if response.stop_reason != "tool_use":
            print("".join(b.text for b in response.content if b.type == "text"))
            return

        results = []
        for block in response.content:
            if block.type != "tool_use":
                continue
            if block.name != TOOL_NAME:
                results.append({"type": "tool_result", "tool_use_id": block.id, "content": f"unknown tool {block.name}", "is_error": True})
                continue
            print(f"--- run_nyra:\n{block.input.get('code', '')}")
            r = box.run(block.input.get("code", ""), block.input.get("stdin") or "")
            print(f"--- result:\n{r.feedback()}\n")
            results.append({"type": "tool_result", "tool_use_id": block.id, "content": r.feedback(), "is_error": not r.ok})
        # every result of this turn in one user message
        messages.append({"role": "user", "content": results})
    print("Stopped after 10 turns.")


if __name__ == "__main__":
    main(" ".join(sys.argv[1:]) or "What is the sum of the squares of the first 1000 odd numbers?")
