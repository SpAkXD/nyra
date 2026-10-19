"""Writes the two prompts of the web A/B test from one brief, so they differ only in the language.

    python bench/web/make_prompts.py                        # brief.md -> prompt_html.md, prompt_nyra.md (here)
    python bench/web/make_prompts.py briefs/dentist.md      # -> prompts/dentist/prompt_html.md, prompt_nyra.md

prompt_html.md: the brief + "write one site.html". prompt_nyra.md: the brief + the two Nyra cards + "write one
site.nyra". Paste either into a chat with any model; the answer is one file.
"""
import pathlib
import re
import sys

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent.parent
COMMENT = re.compile(r"\A<!--.*?-->\n", re.S)


def card(name: str) -> str:
    path = ROOT / "docs" / name
    if not path.is_file():
        return f"(missing: docs/{name})"
    return COMMENT.sub("", path.read_text(encoding="utf-8")).strip()


BRIEF = pathlib.Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else HERE / "brief.md"
OUT = HERE if BRIEF == HERE / "brief.md" else HERE / "prompts" / BRIEF.stem
brief = BRIEF.read_text(encoding="utf-8").strip()

html = f"""You are a senior front-end developer. Build the website described below as **one single file, `site.html`**, that works when opened directly in a browser.

## Rules
- Everything in one file: HTML, all CSS in `<style>`, all JavaScript in `<script>`. No build step.
- No frameworks or libraries (no React, Tailwind, jQuery, animation libraries or icon fonts). Plain HTML, CSS and JavaScript only. Google Fonts is the only external stylesheet allowed; icons, if you want them, are inline SVG.
- The images are the given URLs; do not use other images.
- It must work: no console errors, every interaction in the brief works.

{brief}

## Your answer
Reply with the complete `site.html` in one code block and nothing else: no explanation before or after, no "rest of the code here" placeholders.
"""

nyra = f"""You are a senior developer writing **Nyra**, a small programming language made for AI agents. Nyra builds websites: one `.nyra` file with `use web` is compiled by `nyra build site.nyra` into static HTML, CSS and a little JavaScript. Build the website described below as **one single file, `site.nyra`**.

You have not seen Nyra before. Everything you need is in the two reference cards below: the language card and the web card. Use only what they describe; Nyra is not Python, TypeScript, Rust or JSX.

## Rules
- One file, `site.nyra`, that starts with `use web`. No HTML, CSS or JavaScript of your own unless the web card's escape hatches are truly needed: the built-in components, theme and motion words already give a polished look.
- The images are the given URLs; do not use other images.
- It must compile: follow the cards exactly (names, argument order, named options).

{brief}

---

# Reference 1: the Nyra language card

{card("AGENT_CARD.md")}

---

# Reference 2: the Nyra web card

{card("WEB_CARD.md")}

---

## Your answer
Reply with the complete `site.nyra` in one code block and nothing else: no explanation before or after, no "rest of the code here" placeholders.
"""

OUT.mkdir(parents=True, exist_ok=True)
(OUT / "prompt_html.md").write_text(html, encoding="utf-8", newline="\n")
(OUT / "prompt_nyra.md").write_text(nyra, encoding="utf-8", newline="\n")
print(f"{OUT.relative_to(HERE)}: prompt_html.md {len(html)} chars, prompt_nyra.md {len(nyra)} chars")
