"""Renders a `use web` program into static pages for looking at the kit, without `nyra build`.

    python tools/web_preview.py PROGRAM.nyra OUT_DIR [--nyra PATH] [--img DIR] [--kit DIR] [--link URL]

Runs the program in the interpreter, reads the records it prints (docs/WEB.md, "Build contract")
and writes one HTML file per page into OUT_DIR (`/` -> index.html, `/krby` -> krby.html), with the
whole kit (src/web/kit.css and kit.js, every chunk) and the theme inlined, so a page opens from
the disk (`--link URL` links the kit files instead). Image paths `/img/x` become `img/x`;
`--img DIR` copies DIR into OUT_DIR/img. The fixtures in tests/web/fixtures are made with it:

    python tools/web_preview.py examples/web/kit.nyra tests/web/fixtures --link ../../../src/web/
    python tools/web_preview.py examples/web/elements.nyra tests/web/fixtures --link ../../../src/web/
"""

import argparse
import pathlib
import re
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent


def records(text: str):
    """(kind, header, body) of every record in the program's output."""
    lines = text.split("\n")
    i = 0
    while i < len(lines):
        m = re.fullmatch(r"<!--nyra:(page|css|error) ?(.*?)-->", lines[i])
        i += 1
        if not m:
            continue
        if m.group(1) == "error":
            yield "error", m.group(2), ""
            continue
        body = []
        while i < len(lines) and lines[i] != "<!--nyra:end-->":
            body.append(lines[i])
            i += 1
        i += 1
        yield m.group(1), m.group(2), "\n".join(body)


def page_file(path: str) -> str:
    name = path.strip("/").replace("/", "-")
    return (name or "index") + ".html"


def render(output: str, kit_css: str, kit_js: str, link: str = "") -> dict:
    css = []
    pages = {}
    errors = []
    for kind, head, body in records(output):
        if kind == "css":
            css.append(body)
        elif kind == "error":
            errors.append(head)
        else:
            path = head.split(" ")[0]
            pages[path] = body
    if errors:
        raise SystemExit("the program reported build errors:\n" + "\n".join(errors))
    out = {}
    for path, html in pages.items():
        html = html.replace('"/img/', '"img/').replace("https://flora.ageofwords.site/img/", "img/")
        if link:
            head = f'<link rel="stylesheet" href="{link}kit.css">\n<style>\n' + "\n".join(css) + "\n</style>"
            html = html.replace("<!--nyra:head-->", head)
            html = html.replace("<!--nyra:body-->", f'<script src="{link}kit.js" defer></script>')
        else:
            html = html.replace("<!--nyra:head-->", "<style>\n" + kit_css + "\n" + "\n".join(css) + "\n</style>")
            html = html.replace("<!--nyra:body-->", "<script>\n" + kit_js + "\n</script>")
        out[page_file(path)] = html
    return out


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("program")
    ap.add_argument("out")
    ap.add_argument("--nyra", default=str(ROOT / "target" / "release" / ("nyra.exe" if sys.platform == "win32" else "nyra")))
    ap.add_argument("--img")
    ap.add_argument("--kit", default=str(ROOT / "src" / "web"))
    ap.add_argument("--link", default="", help="link the kit files at this relative URL (\"../../../src/web/\") instead of inlining them")
    a = ap.parse_args()
    run = subprocess.run([a.nyra, "run", a.program, "--interp"], capture_output=True)
    if run.returncode != 0:
        raise SystemExit(run.stderr.decode("utf-8", "replace"))
    kit = pathlib.Path(a.kit)
    pages = render(run.stdout.decode("utf-8").replace("\r\n", "\n"), (kit / "kit.css").read_text(encoding="utf-8"),
                   (kit / "kit.js").read_text(encoding="utf-8"), a.link)
    out = pathlib.Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    for name, html in pages.items():
        (out / name).write_text(html, encoding="utf-8", newline="\n")
    if a.img:
        shutil.copytree(a.img, out / "img", dirs_exist_ok=True)
    print("wrote " + ", ".join(sorted(pages)) + f" to {out}")


if __name__ == "__main__":
    main()
