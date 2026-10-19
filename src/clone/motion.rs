//! The motion of a page for `nyra clone`: what moves, and the Nyra motion word for it (docs/WEB.md,
//! Motion). It reads
//!
//! - scroll-reveal libraries' attributes and classes: AOS (`data-aos="fade-up"`), sal.js,
//!   WOW.js and Animate.css (`wow animate__fadeInUp`), ScrollReveal, and reveal classes
//!   (`reveal`, `fade-up`, ...), plus the start state a server-rendered Framer Motion element has
//!   (`style="opacity:0;transform:translateY(40px)"`);
//! - CSS: `animation` with its `@keyframes` (fade, slide, rise, zoom, pop, spin, pulse, float,
//!   marquee, gradient, ...), `:hover` rules (lift, glow, zoom, underline) and
//!   `background-attachment: fixed` (parallax);
//! - parallax and counter attributes (`data-speed`, `data-rellax-speed`, `data-parallax`,
//!   `data-purecounter-end`, `.counter`, ...);
//! - with `--browser`, what the browser saw (`data-nyra-motion`, `cdp.rs`): elements revealed
//!   while scrolling, numbers that counted up, and every running animation with its keyframes.
//!
//! A keyframes animation that no word fits becomes a custom word: `keyframes(name, body)` in the
//! program, checked as the build checks it (E0388, E0375).

use std::collections::BTreeMap;

use super::css::{Sheet, Styles};
use super::dom::Dom;

/// The motion options of one element (motion words, empty when none).
#[derive(Default, Clone, Debug, PartialEq)]
pub struct Mo {
    pub anim: String,
    pub hover: String,
    pub looping: String,
    pub scroll: String,
}

impl Mo {
    pub fn is_empty(&self) -> bool {
        self.anim.is_empty() && self.hover.is_empty() && self.looping.is_empty() && self.scroll.is_empty()
    }

    /// The options as arguments of a call: `anim: "rise"`, ...
    pub fn args(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (k, v) in [("anim", &self.anim), ("hover", &self.hover), ("loop", &self.looping), ("scroll", &self.scroll)] {
            if !v.is_empty() {
                out.push(format!("{k}: \"{v}\""));
            }
        }
        out
    }

    /// Fills the options this one does not have from `other`.
    pub fn or(&mut self, other: &Mo) {
        for (a, b) in [
            (&mut self.anim, &other.anim),
            (&mut self.hover, &other.hover),
            (&mut self.looping, &other.looping),
            (&mut self.scroll, &other.scroll),
        ] {
            if a.is_empty() {
                *a = b.clone();
            }
        }
    }
}

/// The motion of a page.
#[derive(Default)]
pub struct Motions {
    /// Per node.
    pub of: Vec<Mo>,
    /// The custom keyframes the words use: (name, body), checked.
    pub keyframes: Vec<(String, String)>,
    /// What was found on which element, and the word it became: (node, what, word).
    pub notes: Vec<(usize, String, String)>,
}

impl Motions {
    pub fn get(&self, id: usize) -> &Mo {
        static NONE: Mo = Mo { anim: String::new(), hover: String::new(), looping: String::new(), scroll: String::new() };
        self.of.get(id).unwrap_or(&NONE)
    }
}

/// One `@keyframes` read: its frames as (offset 0..1, declarations).
struct Frames {
    frames: Vec<(f64, Vec<(String, String)>)>,
}

impl Frames {
    fn parse(body: &str) -> Frames {
        let mut frames = Vec::new();
        let mut rest = body;
        while let Some(open) = rest.find('{') {
            let sel = rest[..open].trim();
            let Some(close) = rest[open..].find('}').map(|c| c + open) else { break };
            let decls: Vec<(String, String)> = rest[open + 1..close]
                .split(';')
                .filter_map(|d| d.split_once(':'))
                .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
                .filter(|(k, v)| !k.is_empty() && !v.is_empty())
                .collect();
            for part in sel.split(',') {
                let p = part.trim().to_ascii_lowercase();
                let off = match p.as_str() {
                    "from" => Some(0.0),
                    "to" => Some(1.0),
                    _ => p.trim_end_matches('%').trim().parse::<f64>().ok().map(|v| v / 100.0),
                };
                if let Some(o) = off {
                    frames.push((o, decls.clone()));
                }
            }
            rest = &rest[close + 1..];
        }
        frames.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        Frames { frames }
    }

    /// The values of a quantity per frame (frames that do not set it are left out).
    fn values(&self, f: impl Fn(&[(String, String)]) -> Option<f64>) -> Vec<f64> {
        self.frames.iter().filter_map(|(_, d)| f(d)).collect()
    }

    /// The body again, without vendor prefixes, for `keyframes(name, body)`.
    fn body(&self) -> String {
        let mut parts = Vec::new();
        for (o, d) in &self.frames {
            let decls: Vec<String> =
                d.iter().filter(|(k, _)| !k.starts_with('-')).map(|(k, v)| format!("{k}: {}", v.replace(['{', '}', ';', '"'], ""))).collect();
            if !decls.is_empty() {
                parts.push(format!("{}% {{ {} }}", (o * 100.0).round(), decls.join("; ")));
            }
        }
        parts.join(" ")
    }
}

fn get<'a>(d: &'a [(String, String)], k: &str) -> Option<&'a str> {
    d.iter().rev().find(|(x, _)| x == k).map(|(_, v)| v.as_str())
}

/// A length in px from `12px`, `-50%` (as -50 * `pct`), `1rem`, `10vw` (a 1440 px window).
fn length(v: &str, pct: f64) -> Option<f64> {
    let v = v.trim();
    let num: String = v.chars().take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == '+').collect();
    let n: f64 = num.parse().ok()?;
    let unit = v[num.len()..].trim();
    Some(match unit {
        "" | "px" => n,
        "%" => n * pct,
        "rem" | "em" => n * 16.0,
        "vw" => n * 14.4,
        "vh" => n * 9.0,
        _ => n,
    })
}

/// The arguments of the first `name(...)` in a value.
fn func_args<'a>(v: &'a str, name: &str) -> Option<Vec<&'a str>> {
    let mut from = 0;
    while let Some(at) = v[from..].find(name).map(|a| a + from) {
        let after = &v[at + name.len()..];
        let before_ok = at == 0 || !v[..at].ends_with(|c: char| c.is_ascii_alphanumeric());
        if before_ok && after.starts_with('(') {
            let end = after.find(')')?;
            return Some(after[1..end].split(',').map(str::trim).collect());
        }
        from = at + name.len();
    }
    None
}

/// The (x, y) a frame moves the element by, in px (percentages of a 300 px box).
fn translate(d: &[(String, String)]) -> Option<(f64, f64)> {
    if let Some(t) = get(d, "translate") {
        let p: Vec<&str> = t.split_whitespace().collect();
        if t.trim() == "none" {
            return Some((0.0, 0.0));
        }
        return Some((length(p.first()?, 3.0).unwrap_or(0.0), p.get(1).and_then(|y| length(y, 3.0)).unwrap_or(0.0)));
    }
    let t = get(d, "transform")?;
    if t.trim() == "none" {
        return Some((0.0, 0.0));
    }
    let mut x = 0.0;
    let mut y = 0.0;
    let mut any = false;
    if let Some(a) = func_args(t, "translate3d").or_else(|| func_args(t, "translate")) {
        x = length(a[0], 3.0).unwrap_or(0.0);
        y = a.get(1).and_then(|v| length(v, 3.0)).unwrap_or(0.0);
        any = true;
    }
    if let Some(a) = func_args(t, "translateX") {
        x = length(a[0], 3.0).unwrap_or(0.0);
        any = true;
    }
    if let Some(a) = func_args(t, "translateY") {
        y = length(a[0], 3.0).unwrap_or(0.0);
        any = true;
    }
    if let Some(a) = func_args(t, "matrix") {
        if a.len() == 6 {
            x = a[4].parse().unwrap_or(0.0);
            y = a[5].parse().unwrap_or(0.0);
            any = true;
        }
    }
    (any || !t.contains("translate")).then_some((x, y))
}

/// The percentage of the element's own width a frame moves it by (`translateX(-50%)`).
fn translate_pct(d: &[(String, String)]) -> Option<f64> {
    let t = get(d, "transform").or_else(|| get(d, "translate"))?;
    let a = func_args(t, "translateX").or_else(|| func_args(t, "translate3d")).or_else(|| func_args(t, "translate"));
    let first = match a {
        Some(a) => a[0].to_string(),
        None => t.split_whitespace().next()?.to_string(),
    };
    first.trim().strip_suffix('%')?.trim().parse().ok()
}

fn scale(d: &[(String, String)]) -> Option<f64> {
    if let Some(s) = get(d, "scale") {
        return s.split_whitespace().next()?.parse().ok();
    }
    let t = get(d, "transform")?;
    if t.trim() == "none" {
        return Some(1.0);
    }
    if let Some(a) = func_args(t, "scale3d").or_else(|| func_args(t, "scale")) {
        return a[0].parse().ok();
    }
    if let Some(a) = func_args(t, "matrix") {
        let p: Vec<f64> = a.iter().filter_map(|v| v.parse().ok()).collect();
        if p.len() == 6 {
            return Some((p[0] * p[0] + p[1] * p[1]).sqrt());
        }
    }
    (!t.contains("scale")).then_some(1.0)
}

fn rotate(d: &[(String, String)]) -> Option<f64> {
    let v = get(d, "rotate").map(str::to_string).or_else(|| get(d, "transform").and_then(|t| func_args(t, "rotate").or_else(|| func_args(t, "rotateZ")).map(|a| a[0].to_string())))?;
    let v = v.trim();
    if let Some(n) = v.strip_suffix("deg") {
        return n.trim().parse().ok();
    }
    if let Some(n) = v.strip_suffix("turn") {
        return n.trim().parse::<f64>().ok().map(|t| t * 360.0);
    }
    if let Some(n) = v.strip_suffix("rad") {
        return n.trim().parse::<f64>().ok().map(f64::to_degrees);
    }
    None
}

fn opacity(d: &[(String, String)]) -> Option<f64> {
    get(d, "opacity")?.trim().trim_end_matches('%').parse::<f64>().ok().map(|v| if v > 1.0 { v / 100.0 } else { v })
}

fn spread(v: &[f64]) -> f64 {
    let max = v.iter().cloned().fold(f64::MIN, f64::max);
    let min = v.iter().cloned().fold(f64::MAX, f64::min);
    if v.is_empty() {
        0.0
    } else {
        max - min
    }
}

/// The motion word for a keyframes animation, as (option, word): `loop` for one that never
/// ends, `anim` for one that plays once. `None` when no word fits.
fn classify(body: &str, infinite: bool, duration_ms: f64) -> Option<(&'static str, &'static str)> {
    let f = Frames::parse(body);
    if f.frames.is_empty() {
        return None;
    }
    let xs = f.values(|d| translate(d).map(|t| t.0));
    let ys = f.values(|d| translate(d).map(|t| t.1));
    let ss = f.values(scale);
    let rs = f.values(rotate);
    let os = f.values(opacity);
    let pcts = f.values(translate_pct);
    let bgpos = f.frames.iter().filter(|(_, d)| get(d, "background-position").is_some()).count() >= 2;
    let dash = f.frames.iter().any(|(_, d)| get(d, "stroke-dashoffset").is_some());
    let blur = f.frames.iter().any(|(_, d)| get(d, "filter").is_some_and(|v| v.contains("blur")));
    if infinite {
        if spread(&rs) >= 300.0 {
            return Some(("loop", "spin"));
        }
        if pcts.iter().any(|p| *p <= -30.0) && spread(&ys) < 1.0 || xs.iter().any(|x| *x <= -400.0) {
            return Some(("loop", "marquee"));
        }
        if bgpos {
            return Some(("loop", "gradient"));
        }
        if ss.iter().any(|s| *s >= 1.5) && os.iter().any(|o| *o <= 0.1) {
            return Some(("loop", "ping"));
        }
        if spread(&pcts) >= 150.0 {
            return Some(("loop", "shimmer"));
        }
        if spread(&ys) > 0.5 && spread(&ys) <= 60.0 && spread(&xs) <= 30.0 {
            return Some(("loop", if duration_ms >= 2500.0 { "float" } else { "bob" }));
        }
        if spread(&ss) >= 0.01 || spread(&os) >= 0.15 {
            return Some(("loop", "pulse"));
        }
        if spread(&rs) > 0.5 {
            return Some(("loop", "float"));
        }
        return None;
    }
    if dash {
        return Some(("anim", "draw"));
    }
    let first = &f.frames[0].1;
    let (x0, y0) = translate(first).unwrap_or((0.0, 0.0));
    let s0 = scale(first).unwrap_or(1.0);
    let o0 = opacity(first).unwrap_or(1.0);
    if x0.abs() > y0.abs() && x0.abs() > 4.0 {
        return Some(("anim", if x0 < 0.0 { "slide" } else { "slide-left" }));
    }
    if y0.abs() > 4.0 {
        return Some(("anim", "rise"));
    }
    if s0 < 0.97 {
        return Some(("anim", if ss.iter().any(|s| *s > 1.02) { "pop" } else { "zoom" }));
    }
    if blur {
        return Some(("anim", "blur"));
    }
    if o0 < 0.5 {
        return Some(("anim", "fade"));
    }
    None
}

/// A reveal library's name for an entrance ("fade-up", "fadeInLeft", "zoom-in", "flip-left") as
/// a word.
fn reveal_word(v: &str) -> Option<&'static str> {
    let l = v.to_ascii_lowercase().replace(['_', ' '], "-");
    if l.is_empty() {
        return None;
    }
    let has = |w: &str| l.contains(w);
    Some(if has("flip") || has("bounce") || has("pop") || has("back-in") || has("backin") {
        "pop"
    } else if has("zoom") || has("scale") {
        "zoom"
    } else if has("left") {
        // AOS `fade-left` comes from the right, Animate.css `fadeInLeft` from the left
        if has("inleft") || has("in-left") || has("from-left") || has("slideinleft") {
            "slide"
        } else {
            "slide-left"
        }
    } else if has("right") {
        if has("inright") || has("in-right") || has("from-right") {
            "slide-left"
        } else {
            "slide"
        }
    } else if has("up") || has("down") || has("slide") || has("reveal") || has("rise") {
        "rise"
    } else if has("blur") {
        "blur"
    } else if has("fade") || has("appear") || has("in") {
        "fade"
    } else {
        return None;
    })
}

/// Classes that name a scroll reveal (the element starts hidden, a script shows it).
fn reveal_class(classes: &[&str]) -> Option<(String, &'static str)> {
    for c in classes {
        let l = c.to_ascii_lowercase();
        let animate_css = l.strip_prefix("animate__").filter(|r| !matches!(*r, "animated" | "delay-1s" | "delay-2s" | "faster" | "slow" | "slower" | "fast"));
        let named = [
            "reveal", "reveal-up", "scroll-reveal", "js-reveal", "sr", "sr-item", "fade-in", "fade-up", "fade-in-up", "fadein", "fade-in-bottom",
            "animate-on-scroll", "anim-up", "slide-up", "slide-in", "appear", "zoom-in", "fadeinup", "fadeinleft", "fadeinright", "fadeindown", "fadein",
            "zoomin", "slideinup", "slideinleft", "slideinright", "bouncein", "gsap-reveal", "gs-reveal", "inview", "in-view-animate",
        ];
        if let Some(r) = animate_css {
            if let Some(w) = reveal_word(r) {
                return Some((format!("class `{c}`"), w));
            }
        }
        if named.contains(&l.as_str()) || (l.starts_with("wow") && l.len() == 3) {
            let w = reveal_word(&l).unwrap_or("rise");
            return Some((format!("class `{c}`"), w));
        }
    }
    None
}

fn counter_like(dom: &Dom, id: usize, classes: &[&str]) -> Option<String> {
    for a in ["data-purecounter-end", "data-count", "data-countup", "data-count-to", "data-to", "data-target", "data-number", "data-value"] {
        if let Some(v) = dom.attr(id, a) {
            if v.trim().replace([',', '.', ' '], "").parse::<f64>().is_ok() && dom.nodes[id].kids.iter().any(|&k| dom.tag(k) == "#text") {
                return Some(format!("`{a}`"));
            }
        }
    }
    classes
        .iter()
        .find(|c| {
            let l = c.to_ascii_lowercase();
            ["counter", "count-up", "countup", "odometer", "purecounter", "js-count", "number-count", "stat-count"].iter().any(|w| l == *w || l.ends_with(&format!("-{w}")) || l.starts_with(&format!("{w}-")))
        })
        .map(|c| format!("class `{c}`"))
}

/// Finds the motion of every element.
pub fn detect(dom: &Dom, st: &Styles, sheet: &Sheet) -> Motions {
    let mut m = Motions { of: vec![Mo::default(); dom.nodes.len()], ..Default::default() };
    // the keyframes the page has: its stylesheets', and those the browser reported
    let mut frames: Vec<(String, String)> = sheet.keyframes.clone();
    for id in dom.find_all(0, "meta") {
        if dom.attr(id, "name") == Some("nyra-keyframes") {
            if let Some((n, b)) = dom.attr(id, "content").and_then(|c| c.split_once('\n')) {
                if !frames.iter().any(|(x, _)| x == n) {
                    frames.push((n.to_string(), b.to_string()));
                }
            }
        }
    }
    let mut custom: Vec<(String, String)> = Vec::new();
    let mut word_of_keyframes = |name: &str, infinite: bool, ms: f64, found: &mut BTreeMap<(String, String), usize>| -> Option<(&'static str, String)> {
        let body = frames.iter().find(|(n, _)| n == name).map(|(_, b)| b.clone())?;
        let what = format!("@keyframes {name}{}", if infinite { " (endless)" } else { "" });
        if let Some((opt, w)) = classify(&body, infinite, ms) {
            *found.entry((what, format!("{opt}: \"{w}\""))).or_default() += 1;
            return Some((opt, w.to_string()));
        }
        // a word of its own: the keyframes in the program
        let opt = if infinite { "loop" } else { "anim" };
        let word = custom_name(name);
        if let Some((_, _)) = custom.iter().find(|(n, _)| *n == word) {
            *found.entry((what, format!("{opt}: \"{word}\" (keyframes)"))).or_default() += 1;
            return Some((opt, word));
        }
        if custom.len() >= 6 {
            return None;
        }
        let clean = Frames::parse(&body).body();
        // only keyframes the build takes without a warning (no layout properties)
        match crate::web::motion::check_keyframes(&word, &clean) {
            Ok(kf) if !clean.is_empty() && clean.len() <= 600 && kf.layout_warning().is_none() => {}
            _ => return None,
        }
        custom.push((word.clone(), clean));
        *found.entry((what, format!("{opt}: \"{word}\" (keyframes)"))).or_default() += 1;
        Some((opt, word))
    };
    for id in 0..dom.nodes.len() {
        if !dom.is_elem(id) || matches!(dom.tag(id), "html" | "head" | "body" | "script" | "style" | "meta" | "link") {
            continue;
        }
        let classes = dom.classes(id);
        let mut mo = Mo::default();
        let mut here: BTreeMap<(String, String), usize> = BTreeMap::new();
        let note = |found: &mut BTreeMap<(String, String), usize>, what: String, mo_part: String| {
            *found.entry((what, mo_part)).or_default() += 1;
        };
        // what the browser saw
        if let Some(seen) = dom.attr(id, "data-nyra-motion") {
            for part in seen.split(';') {
                let Some((opt, v)) = part.split_once(':') else { continue };
                if let Some(rest) = v.strip_prefix('@') {
                    let (name, ms) = rest.rsplit_once('@').unwrap_or((rest, "0"));
                    let ms: f64 = ms.parse().unwrap_or(0.0);
                    if let Some((o, w)) = word_of_keyframes(name, opt == "loop", ms, &mut here) {
                        set(&mut mo, o, &w);
                    }
                } else if opt == "anim" {
                    let what = if v == "count" { "a number that counted up (browser)" } else { "revealed while scrolling (browser)" };
                    note(&mut here, what.to_string(), format!("anim: \"{v}\""));
                    set(&mut mo, "anim", v);
                }
            }
        }
        // reveal libraries
        for a in ["data-aos", "data-sal", "data-animate", "data-animation", "data-scroll-reveal", "data-sr", "data-reveal"] {
            if let Some(v) = dom.attr(id, a) {
                if let Some(w) = reveal_word(v).or_else(|| (a == "data-sr" || a == "data-reveal").then_some("rise")) {
                    if mo.anim.is_empty() {
                        note(&mut here, format!("{a}=\"{v}\""), format!("anim: \"{w}\""));
                        mo.anim = w.into();
                    }
                }
            }
        }
        if mo.anim.is_empty() {
            if let Some((what, w)) = reveal_class(&classes) {
                note(&mut here, what, format!("anim: \"{w}\""));
                mo.anim = w.into();
            }
        }
        // a start state written into the element (server-rendered Framer Motion, GSAP)
        if mo.anim.is_empty() {
            if let Some(style) = dom.attr(id, "style") {
                let s = style.replace(' ', "").to_ascii_lowercase();
                if s.contains("opacity:0;") || s.ends_with("opacity:0") {
                    let w = if s.contains("translatey(") || s.contains("translate3d(0") || s.contains("translate(0") {
                        "rise"
                    } else if s.contains("translatex(-") {
                        "slide"
                    } else if s.contains("translatex(") {
                        "slide-left"
                    } else if s.contains("scale(") {
                        "zoom"
                    } else {
                        "fade"
                    };
                    note(&mut here, "a start state in `style` (opacity 0)".into(), format!("anim: \"{w}\""));
                    mo.anim = w.into();
                }
            }
        }
        // counters
        if mo.anim.is_empty() {
            if let Some(what) = counter_like(dom, id, &classes) {
                note(&mut here, format!("counter ({what})"), "anim: \"count\"".into());
                mo.anim = "count".into();
            }
        }
        // typing effects and split text
        if mo.anim.is_empty() && classes.iter().any(|c| matches!(c.to_ascii_lowercase().as_str(), "typed" | "typewriter" | "typing" | "type-effect")) {
            note(&mut here, "a typing effect".into(), "anim: \"letters\"".into());
            mo.anim = "letters".into();
        }
        if mo.anim.is_empty()
            && (dom.attr(id, "data-splitting").is_some() || classes.iter().any(|c| matches!(c.to_ascii_lowercase().as_str(), "split-text" | "splitting" | "split-words")))
        {
            note(&mut here, "split text".into(), "anim: \"words\"".into());
            mo.anim = "words".into();
        }
        // parallax
        let parallax_attr = ["data-speed", "data-scroll-speed", "data-rellax-speed", "data-parallax", "data-jarallax", "data-stellar-background-ratio"]
            .iter()
            .find(|a| dom.attr(id, a).is_some());
        let parallax_class = classes.iter().find(|c| matches!(c.to_ascii_lowercase().as_str(), "parallax" | "rellax" | "jarallax" | "parallax-bg" | "parallax-window"));
        if let Some(a) = parallax_attr {
            note(&mut here, format!("parallax (`{a}`)"), "scroll: \"parallax\"".into());
            mo.scroll = "parallax".into();
        } else if let Some(c) = parallax_class {
            note(&mut here, format!("parallax (class `{c}`)"), "scroll: \"parallax\"".into());
            mo.scroll = "parallax".into();
        } else if st.own(dom, id, "background-attachment").is_some_and(|v| v.contains("fixed")) {
            note(&mut here, "parallax (`background-attachment: fixed`)".into(), "scroll: \"parallax\"".into());
            mo.scroll = "parallax".into();
        }
        // CSS animations
        if let Some(anim) = st.own(dom, id, "animation-name").or_else(|| st.own(dom, id, "animation")) {
            let infinite = anim.contains("infinite") || st.own(dom, id, "animation-iteration-count").is_some_and(|v| v.contains("infinite"));
            let ms = anim
                .split(|c: char| c.is_whitespace() || c == ',')
                .chain(st.own(dom, id, "animation-duration").as_deref().unwrap_or("").split_whitespace())
                .find_map(|t| t.strip_suffix("ms").and_then(|n| n.parse::<f64>().ok()).or_else(|| t.strip_suffix('s').and_then(|n| n.parse::<f64>().ok()).map(|s| s * 1000.0)))
                .unwrap_or(1000.0);
            let names: Vec<String> = anim.split(|c: char| c.is_whitespace() || c == ',').filter(|t| frames.iter().any(|(n, _)| n == t)).map(str::to_string).collect();
            if let Some(name) = names.first() {
                if let Some((o, w)) = word_of_keyframes(name, infinite, ms, &mut here) {
                    if (o == "anim" && mo.anim.is_empty()) || (o == "loop" && mo.looping.is_empty()) {
                        set(&mut mo, o, &w);
                    }
                }
            }
        }
        // :hover rules
        let hov = st.hover(id);
        if !hov.is_empty() {
            let inner = hov.iter().any(|(k, _)| k == "--nyra-inner");
            let has = |k: &str| hov.iter().any(|(x, _)| x == k);
            let val = |k: &str| hov.iter().rev().find(|(x, _)| x == k).map(|(_, v)| v.as_str()).unwrap_or("");
            let t = format!("{} {}", val("transform"), val("translate"));
            let up = t.contains("translateY(-") || t.contains("translate(0,-") || t.contains("translate(0, -") || t.contains("translate3d(0,-") || t.contains("translate3d(0, -") || val("translate").trim().split_whitespace().nth(1).is_some_and(|y| y.starts_with('-')) || val("top").starts_with('-') || val("margin-top").starts_with('-');
            let grows = t.contains("scale(") || has("scale");
            let word = if up {
                Some(("translateY up", "lift"))
            } else if grows && (inner || dom.tag(id) == "img") {
                Some(("scale of a picture", "zoom"))
            } else if grows {
                Some(("scale", "lift"))
            } else if has("box-shadow") {
                Some(("box-shadow", "glow"))
            } else if (val("text-decoration").contains("underline") || val("text-decoration-line").contains("underline") || has("background-size")) && matches!(dom.tag(id), "a" | "span" | "button") {
                Some(("underline", "underline"))
            } else {
                None
            };
            if let Some((what, w)) = word {
                note(&mut here, format!(":hover {what}"), format!("hover: \"{w}\""));
                mo.hover = w.into();
            }
        }
        // a `<marquee>` element
        if dom.tag(id) == "marquee" {
            mo.looping = "marquee".into();
            note(&mut here, "<marquee>".into(), "loop: \"marquee\"".into());
        }
        m.of[id] = mo;
        m.notes.extend(here.into_keys().map(|(what, word)| (id, what, word)));
    }
    m.keyframes = custom;
    m
}

fn set(mo: &mut Mo, opt: &str, w: &str) {
    match opt {
        "anim" => mo.anim = w.to_string(),
        "loop" => mo.looping = w.to_string(),
        "hover" => mo.hover = w.to_string(),
        "scroll" => mo.scroll = w.to_string(),
        _ => {}
    }
}

/// A keyframes name usable as a word: letters, digits and dashes, starting with a letter, not a
/// built-in word.
fn custom_name(name: &str) -> String {
    let mut s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c.to_ascii_lowercase() } else { '-' }).collect();
    s = s.trim_matches('-').to_string();
    if !s.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
        s = format!("k-{s}");
    }
    let builtin = crate::web::motion::ANIM.iter().chain(crate::web::motion::HOVER).chain(crate::web::motion::LOOP).chain(crate::web::motion::SCROLL);
    if builtin.clone().any(|w| *w == s) || ["fast", "slow", "spring"].contains(&s.as_str()) {
        s = format!("site-{s}");
    }
    s.chars().take(40).collect::<String>().trim_end_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyframes_become_words() {
        assert_eq!(classify("from { opacity: 0; transform: translateY(30px) } to { opacity: 1; transform: none }", false, 600.0), Some(("anim", "rise")));
        assert_eq!(classify("0% { opacity: 0; transform: translateX(-40px) } 100% { opacity: 1 }", false, 600.0), Some(("anim", "slide")));
        assert_eq!(classify("from { opacity: 0 } to { opacity: 1 }", false, 400.0), Some(("anim", "fade")));
        assert_eq!(classify("0% { transform: scale(.5) } 70% { transform: scale(1.1) } 100% { transform: scale(1) }", false, 400.0), Some(("anim", "pop")));
        assert_eq!(classify("to { transform: rotate(360deg) }", true, 2000.0), Some(("loop", "spin")));
        assert_eq!(classify("0% { transform: translateX(0) } 100% { transform: translateX(-50%) }", true, 20000.0), Some(("loop", "marquee")));
        assert_eq!(classify("0%,100% { transform: translateY(0) } 50% { transform: translateY(-12px) }", true, 4000.0), Some(("loop", "float")));
        assert_eq!(classify("0%,100% { transform: scale(1) } 50% { transform: scale(1.06) }", true, 2000.0), Some(("loop", "pulse")));
        assert_eq!(classify("0% { background-position: 0% 50% } 50% { background-position: 100% 50% } 100% { background-position: 0% 50% }", true, 8000.0), Some(("loop", "gradient")));
        assert_eq!(classify("0% { width: 10px } 100% { width: 50px }", false, 400.0), None);
    }

    #[test]
    fn reveal_names() {
        assert_eq!(reveal_word("fade-up"), Some("rise"));
        assert_eq!(reveal_word("fade-left"), Some("slide-left"));
        assert_eq!(reveal_word("fadeInLeft"), Some("slide"));
        assert_eq!(reveal_word("zoom-in"), Some("zoom"));
        assert_eq!(reveal_word("flip-up"), Some("pop"));
        assert_eq!(reveal_word("fade"), Some("fade"));
        assert_eq!(custom_name("float"), "site-float");
        assert_eq!(custom_name("myWiggle"), "mywiggle");
    }
}
