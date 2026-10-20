"""Writes src/std/web_icons.nyra: the icon set of `use web`, a subset of Lucide (ISC license).

    python tools/gen_web_icons.py PATH/TO/node_modules/lucide-react
    python tools/gen_web_icons.py --keep      (the bodies of the current file: for new aliases and names)

Reads each icon's elements from `dist/esm/icons/<name>.mjs` of lucide-react and writes them as one
Nyra `match` (name -> the SVG elements). The SVG wrapper (viewBox 0 0 24 24, stroke, round caps) is
written by the renderer and the kit's CSS. Old Lucide names that were renamed are kept as aliases.
Names from other icon sets (`envelope`, `map-marker`) become `_icon_like`: the build draws the
Lucide icon they stand for, with warning E0389. Brand icons are not in Lucide: the few here are from
Simple Icons (CC0), written out below.
"""

import json
import pathlib
import re
import sys

# The icons of the kit: what business and portfolio sites use most. Keep it sorted by theme.
ICONS = """
phone phone-call smartphone mail mail-open send at-sign message-circle message-square map-pin map
navigation compass globe clock calendar calendar-days timer hourglass history
house building building-2 store warehouse factory hotel school church landmark tent
leaf flower flower-2 sprout tree-pine trees sun moon cloud cloud-sun snowflake droplet droplets
wind flame thermometer mountain waves umbrella zap
ruler hard-hat hammer wrench drill paintbrush paint-roller palette pen-tool pencil scissors
shovel axe brick-wall construction
search medal award trophy star sparkles crown gem gift heart thumbs-up badge-check shield
shield-check lock key handshake graduation-cap book-open briefcase
user users baby dog cat bike car truck bus train-front plane ship fuel
shopping-bag shopping-cart package credit-card banknote wallet receipt tag percent euro
dollar-sign coins piggy-bank chart-column chart-line trending-up target rocket lightbulb
coffee cup-soda utensils chef-hat pizza cake-slice wine beer apple carrot cherry croissant
wheat cookie donut sandwich egg ice-cream-cone candy cake
scale stethoscope pill activity dumbbell syringe
camera image images video film play music headphones mic tv monitor laptop wifi
code terminal cpu database server cloud-upload download upload link link-2 external-link
share-2 printer file-text clipboard-list list-checks check check-check circle-check circle-x
x plus minus info circle-help circle-alert triangle-alert circle bell bookmark eye settings sliders-horizontal
layout-grid layers grid-3x3 menu ellipsis
arrow-right arrow-left arrow-up arrow-down arrow-up-right move-right chevron-right
chevron-left chevron-up chevron-down chevrons-down expand maximize-2 minimize-2 zoom-in
quote recycle accessibility languages hand-heart smile party-popper flag
""".split()

# Lucide renamed these; the old names keep working.
ALIASES = {
    "home": "house",
    "check-circle": "circle-check",
    "check-circle-2": "circle-check",
    "x-circle": "circle-x",
    "help-circle": "circle-help",
    "alert-circle": "circle-alert",
    "alert-triangle": "triangle-alert",
    "bar-chart": "chart-column",
    "bar-chart-2": "chart-column",
    "bar-chart-3": "chart-column",
    "line-chart": "chart-line",
    "train": "train-front",
    "more-horizontal": "ellipsis",
    "grid": "grid-3x3",
    "verified": "badge-check",
}

# The same shapes as Lucide's, for `--keep` (a name new to the list when lucide-react is not at hand).
KNOWN = {
    "circle": "<circle cx='12' cy='12' r='10'/>",
    "circle-alert": "<circle cx='12' cy='12' r='10'/><path d='M12 8v4'/><path d='M12 16h.01'/>",
}

# Names from other icon sets (Font Awesome, Material, Feather, Heroicons) and plain words for the
# same symbol: not Lucide names, but certain about the icon meant. The build draws that icon and
# warns (E0389) with the Lucide name to write.
LIKE = {
    "envelope": "mail", "email": "mail", "e-mail": "mail", "envelope-o": "mail", "mail-alt": "mail",
    "envelope-open": "mail-open", "paper-plane": "send",
    "telephone": "phone", "phone-alt": "phone", "call": "phone", "phone-square": "phone",
    "mobile": "smartphone", "mobile-alt": "smartphone", "mobile-phone": "smartphone", "cellphone": "smartphone",
    "map-marker": "map-pin", "map-marker-alt": "map-pin", "location": "map-pin", "location-dot": "map-pin",
    "location-pin": "map-pin", "location-on": "map-pin", "place": "map-pin",
    "map-alt": "map", "directions": "navigation", "location-arrow": "navigation",
    "clock-o": "clock", "time": "clock", "access-time": "clock", "schedule": "clock",
    "calendar-alt": "calendar", "calendar-o": "calendar", "event": "calendar", "date": "calendar",
    "calendar-week": "calendar-days", "stopwatch": "timer", "hourglass-half": "hourglass",
    "home-alt": "house", "house-chimney": "house",
    "person": "user", "account": "user", "user-alt": "user", "user-o": "user",
    "group": "users", "people": "users", "team": "users", "user-friends": "users", "users-alt": "users",
    "cart": "shopping-cart", "cart-shopping": "shopping-cart", "basket": "shopping-bag", "bag": "shopping-bag",
    "warning": "triangle-alert", "alert": "triangle-alert", "exclamation-triangle": "triangle-alert",
    "triangle-exclamation": "triangle-alert", "exclamation-circle": "circle-alert", "circle-exclamation": "circle-alert",
    "error": "circle-alert", "info-circle": "info", "circle-info": "info",
    "question": "circle-help", "question-circle": "circle-help", "circle-question": "circle-help", "help": "circle-help",
    "tick": "check", "checkmark": "check", "done": "check", "check-square": "check",
    "check-circle-o": "circle-check", "circle-check-big": "circle-check", "task-alt": "circle-check",
    "close": "x", "times": "x", "xmark": "x", "cross": "x",
    "magnifying-glass": "search", "magnifier": "search",
    "cog": "settings", "gear": "settings", "cogs": "settings", "sliders": "sliders-horizontal", "sliders-h": "sliders-horizontal",
    "bars": "menu", "hamburger": "menu", "navicon": "menu",
    "star-o": "star", "heart-o": "heart", "thumbs-o-up": "thumbs-up", "like": "thumbs-up",
    "money": "banknote", "cash": "banknote", "money-bill": "banknote", "dollar": "dollar-sign",
    "euro-sign": "euro", "credit-card-alt": "credit-card", "payment": "credit-card",
    "shipping": "truck", "shipping-fast": "truck", "local-shipping": "truck", "delivery": "truck",
    "facebook-f": "facebook", "facebook-square": "facebook",
    "world": "globe", "web": "globe", "website": "globe", "globe-alt": "globe", "language": "languages",
    "translate": "languages", "link-alt": "link", "external-link-alt": "external-link", "share-alt": "share-2",
    "comment": "message-circle", "comments": "message-square", "chat": "message-circle", "message": "message-square",
    "quote-left": "quote", "quote-right": "quote",
    "fire": "flame", "bolt": "zap", "lightning": "zap", "flash": "zap",
    "restaurant": "utensils", "cutlery": "utensils", "fork-knife": "utensils",
    "photo": "image", "picture": "image", "camera-alt": "camera", "video-camera": "video",
    "music-note": "music", "headset": "headphones", "desktop": "monitor", "computer": "monitor",
    "print": "printer", "file-alt": "file-text", "document": "file-text", "description": "file-text",
    "list-check": "list-checks", "eye-open": "eye", "visibility": "eye", "bell-o": "bell", "notifications": "bell",
    "th": "layout-grid", "th-large": "layout-grid", "grid-alt": "layout-grid", "apps": "layout-grid",
    "ellipsis-h": "ellipsis", "more": "ellipsis", "more-horiz": "ellipsis",
    "sun-o": "sun", "moon-o": "moon", "water": "droplet", "drop": "droplet", "tint": "droplet",
    "thermometer-half": "thermometer", "tools": "wrench", "build": "wrench", "paint-brush": "paintbrush",
    "pencil-alt": "pencil", "edit": "pencil", "pen": "pencil", "cut": "scissors", "seedling": "sprout",
    "diamond": "gem", "hand-holding-heart": "hand-heart", "smile-o": "smile", "face-smile": "smile",
    "wheelchair": "accessibility", "universal-access": "accessibility",
    "car-side": "car", "bus-alt": "bus", "airplane": "plane", "plane-departure": "plane", "flight": "plane",
    "boat": "ship", "bicycle": "bike", "gas-pump": "fuel", "shop": "store", "store-alt": "store",
    "office": "building", "city": "building-2", "university": "school", "campground": "tent",
    "wine-glass": "wine", "pizza-slice": "pizza", "birthday-cake": "cake", "cookie-bite": "cookie",
    "ice-cream": "ice-cream-cone", "apple-alt": "apple", "candy-cane": "candy",
    "pills": "pill", "capsules": "pill", "heartbeat": "activity", "heart-pulse": "activity",
    "balance-scale": "scale", "tags": "tag", "percentage": "percent", "chart-bar": "chart-column",
    "trending": "trending-up", "arrow-trend-up": "trending-up", "bullseye": "target",
    "bulb": "lightbulb", "idea": "lightbulb", "microchip": "cpu", "cloud-upload-alt": "cloud-upload",
    "at": "at-sign", "history-alt": "history", "trophy-alt": "trophy",
}

# Brand icons from Simple Icons (https://simpleicons.org, CC0): filled, not stroked.
BRANDS = {
    "facebook": "M24 12.073c0-6.627-5.373-12-12-12s-12 5.373-12 12c0 5.99 4.388 10.954 10.125 11.854v-8.385H7.078v-3.47h3.047V9.43c0-3.007 1.792-4.669 4.533-4.669 1.312 0 2.686.235 2.686.235v2.953H15.83c-1.491 0-1.956.925-1.956 1.874v2.25h3.328l-.532 3.47h-2.796v8.385C19.612 23.027 24 18.062 24 12.073z",
}

NOTICE = """\
// The icons of `use web`: a subset of Lucide (https://lucide.dev), plus brand icons from
// Simple Icons (https://simpleicons.org, CC0 1.0).
//
// Lucide is under the ISC License:
//   Copyright (c) for portions of Lucide are held by Cole Bemis 2013-2022 as part of Feather (MIT).
//   All other copyright (c) for Lucide are held by Lucide Contributors 2022.
//   Permission to use, copy, modify, and/or distribute this software for any purpose with or
//   without fee is hereby granted, provided that the above copyright notice and this permission
//   notice appear in all copies.
//   THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH REGARD TO THIS
//   SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE
//   AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
//   WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT,
//   NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR
//   PERFORMANCE OF THIS SOFTWARE.
//
// Generated by tools/gen_web_icons.py from lucide-react {version}: change the script, not this file.
"""


def elements(path: pathlib.Path) -> str:
    """The SVG elements of one icon module, as markup with single-quoted attributes."""
    src = path.read_text(encoding="utf-8")
    # a renamed icon re-exports the new one
    again = re.search(r"export \{ default \} from '\./([\w-]+\.mjs)'", src)
    if again:
        return elements(path.parent / again.group(1))
    m = re.search(r"const __iconNode = (\[.*?\]);\n", src, re.S)
    if not m:
        raise SystemExit(f"no icon node in {path}")
    # the node is a JS literal: quote the keys to read it as JSON
    text = re.sub(r"(\s)([a-zA-Z][\w-]*):", r'\1"\2":', m.group(1))
    out = []
    for tag, attrs in json.loads(text):
        parts = [f"{k}='{v}'" for k, v in attrs.items() if k != "key"]
        out.append(f"<{tag} {' '.join(parts)}/>")
    return "".join(out)


OUT = pathlib.Path(__file__).resolve().parent.parent / "src" / "std" / "web_icons.nyra"


def current_bodies() -> tuple[str, dict]:
    """The lucide-react version and the icon bodies of the file as it is (for `--keep`)."""
    text = OUT.read_text(encoding="utf-8")
    version = re.search(r"from lucide-react (\S+):", text).group(1)
    part = text.split("fn _icon_body(name: str) -> str = match name {", 1)[1].split("\n}", 1)[0]
    bodies = dict(re.findall(r'^    "([a-z0-9-]+)" => "(.*)"$', part, re.M))
    return version, bodies


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    bodies = {}
    if sys.argv[1] == "--keep":
        version, have = current_bodies()
        for name in ICONS:
            if name not in have and name not in KNOWN:
                raise SystemExit(f"no body for `{name}`: run with the path of lucide-react")
            bodies[name] = have.get(name) or KNOWN[name]
    else:
        pkg = pathlib.Path(sys.argv[1])
        version = json.loads((pkg / "package.json").read_text(encoding="utf-8"))["version"]
        icons = pkg / "dist" / "esm" / "icons"
        for name in ICONS:
            f = icons / f"{name}.mjs"
            if not f.exists():
                raise SystemExit(f"lucide-react {version} has no icon `{name}`")
            bodies[name] = elements(f)
    for old, new in ALIASES.items():
        if new not in bodies:
            raise SystemExit(f"the alias `{old}` names `{new}`, which is not in ICONS")
    for other, new in LIKE.items():
        if new not in bodies and new not in BRANDS:
            raise SystemExit(f"`{other}` stands for `{new}`, which is not in ICONS")
        if other in bodies or other in ALIASES or other in BRANDS:
            raise SystemExit(f"`{other}` is an icon of the set: it cannot stand for another")
    lines = [NOTICE.format(version=version)]
    # the names alone, so that rendering an icon (a <use> of the page's sprite) needs no paths
    names = sorted(list(bodies) + list(ALIASES))
    lines.append("// 1 for a stroked icon, 2 for a brand icon, 0 for a name that is no icon.")
    lines.append("fn _icon_kind(name: str) -> int {")
    lines.append(f'    let icons = "|{"|".join(names)}|"')
    lines.append(f'    let brands = "|{"|".join(sorted(BRANDS))}|"')
    lines.append('    let key = "|" + name + "|"')
    lines.append("    return if icons.contains(key) { 1 } else if brands.contains(key) { 2 } else { 0 }")
    lines.append("}")
    lines.append("")
    lines.append("// The SVG elements of the icon `name` (24 x 24, stroked), \"\" for a name that is no icon.")
    lines.append("fn _icon_body(name: str) -> str = match name {")
    for name in sorted(bodies):
        lines.append(f'    "{name}" => "{bodies[name]}"')
    for old in sorted(ALIASES):
        lines.append(f'    "{old}" => "{bodies[ALIASES[old]]}"')
    lines.append('    _ => ""')
    lines.append("}")
    lines.append("")
    lines.append("// The path of the brand icon `name` (24 x 24, filled), \"\" for a name that is no brand icon.")
    lines.append("fn _brand_body(name: str) -> str = match name {")
    for name in sorted(BRANDS):
        lines.append(f'    "{name}" => "<path d=\'{BRANDS[name]}\'/>"')
    lines.append('    _ => ""')
    lines.append("}")
    lines.append("")
    lines.append("// The icon of the set that a name of another icon set stands for (warning E0389), \"\" for none.")
    lines.append("// (One string, not a `match`: this is also compiled into the code of live views for the browser.)")
    lines.append("fn _icon_like(name: str) -> str {")
    lines.append(f'    let pairs = "|{"|".join(f"{k}={LIKE[k]}" for k in sorted(LIKE))}|"')
    lines.append('    let at = pairs.index_of("|" + name + "=")')
    lines.append("    if at < 0 {")
    lines.append('        return ""')
    lines.append("    }")
    lines.append("    let rest = pairs.slice(at + name.len() + 2, pairs.len())")
    lines.append('    return rest.slice(0, rest.index_of("|"))')
    lines.append("}")
    OUT.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    print(f"wrote {OUT}: {len(bodies)} icons, {len(ALIASES)} aliases, {len(LIKE)} names of other sets, {len(BRANDS)} brand icons")


if __name__ == "__main__":
    main()
