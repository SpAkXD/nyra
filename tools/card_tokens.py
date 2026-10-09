"""Measure docs/AGENT_CARD.md on Claude's tokenizer (the free count_tokens endpoint) and keep its header honest.

    python tools/card_tokens.py            # print the token counts and the budget
    python tools/card_tokens.py --write    # also write the counts into the card's header comment

The card has a hard budget (BUDGET below): a feature that needs card text must displace something.
Needs ANTHROPIC_API_KEY in the environment; the key is only read by the anthropic SDK and never printed.
Counts are of the served card (the file without its header comment), the per-message overhead of the endpoint subtracted.
"""
import argparse
import re
import sys
from pathlib import Path

CARD = Path(__file__).resolve().parent.parent / "docs" / "AGENT_CARD.md"
BUDGET = 1400
MODELS = ("claude-sonnet-5-5", "claude-haiku-4-5-20251001")
COMMENT = re.compile(r"\A<!--.*?-->\n", re.S)  # the metadata header: never served, never counted
TOKENS = re.compile(r"TOKENS: \d+ on claude-sonnet-5-5, \d+ on claude-haiku-4-5")


def body(text: str) -> str:
    """The card as a model sees it: the file without its header comment."""
    return COMMENT.sub("", text, count=1)


def count(client, model: str, text: str) -> int:
    def raw(t):
        return client.messages.count_tokens(model=model, messages=[{"role": "user", "content": t}]).input_tokens
    return raw(text) - (raw("x") - 1)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--write", action="store_true", help="write the counts into the header comment")
    args = ap.parse_args()
    import anthropic
    client = anthropic.Anthropic(max_retries=6)
    text = CARD.read_text(encoding="utf-8").replace("\r\n", "\n")
    counts = {m: count(client, m, body(text)) for m in MODELS}
    for m, n in counts.items():
        print(f"{m:28} {n:5} tokens  ({len(body(text))} chars)")
    ok = max(counts.values()) <= BUDGET
    print(f"budget {BUDGET}: " + ("ok" if ok else "OVER BUDGET: cut text"))
    if args.write:
        line = f"TOKENS: {counts[MODELS[0]]} on claude-sonnet-5-5, {counts[MODELS[1]]} on claude-haiku-4-5"
        CARD.write_text(TOKENS.sub(line, text, count=1), encoding="utf-8", newline="\n")
        print("header written")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
