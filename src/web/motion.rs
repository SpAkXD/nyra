//! The motion words of `use web` (`docs/WEB.md`, Motion): what they mean, the CSS and JS a page
//! needs for the words it uses, and the checks of `keyframes(name, body)`.
//!
//! The renderer writes each motion option as an attribute with the value as written
//! (`anim: "rise slow 0.2"` becomes `data-anim="rise slow 0.2"`), plus `data-at`, `data-motion`
//! and `data-morph`. `motion.css` and `motion.js` key off those attributes only. Both files are
//! split into parts, each starting with a line `/*! motion: <part> */`; the build keeps the parts
//! the pages use ([`motion_css`], [`motion_js`]), and adds a few generated rules for the values
//! that carry numbers (a delay, a duration, a spring, a parallax rate, `data-at`).
//!
//! Checks never give error codes here: [`parse`], [`check_keyframes`] and [`lint`] say what is
//! wrong and the caller turns that into a diagnostic (the codes are listed in `docs/WEB.md`,
//! "Motion build contract").

// The build (WP5) and the checker hooks (WP2) call this module; until they land only the tests do.
#![allow(dead_code)]

use super::ease::{self, Ease, Spring};
use crate::ast::Span;
use crate::lexer::{Tok, Token};

const CSS: &str = include_str!("motion.css");
const JS: &str = include_str!("motion.js");

/// `anim:` words: once, when the element scrolls into view.
pub const ANIM: &[&str] =
    &["fade", "rise", "slide", "slide-left", "zoom", "blur", "pop", "words", "letters", "draw", "count", "stagger", "none"];
/// `hover:` words: pointer over or keyboard focus.
pub const HOVER: &[&str] = &["lift", "glow", "zoom", "shine", "tilt", "underline", "fill", "magnetic"];
/// `loop:` words: always, while on screen.
pub const LOOP: &[&str] = &["bob", "float", "pulse", "ping", "spin", "shimmer", "gradient", "marquee"];
/// `scroll:` words: linked to the scroll position.
pub const SCROLL: &[&str] = &["parallax", "fade-out", "scale", "progress"];
/// Speed modifiers.
pub const SPEEDS: &[&str] = &["fast", "slow"];
/// `site(transition:)`.
pub const TRANSITIONS: &[&str] = &["fade", "slide", "rise", "none"];
/// `site(motion:)`, written as `data-motion` on `<html>`.
pub const LEVELS: &[&str] = &["full", "calm", "off"];

/// One of the four motion options every element takes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Opt {
    Anim,
    Hover,
    Loop,
    Scroll,
}

impl Opt {
    pub const ALL: [Opt; 4] = [Opt::Anim, Opt::Hover, Opt::Loop, Opt::Scroll];

    /// `anim`, also from the attribute name `data-anim`.
    pub fn from_name(name: &str) -> Option<Opt> {
        Some(match name.strip_prefix("data-").unwrap_or(name) {
            "anim" => Opt::Anim,
            "hover" => Opt::Hover,
            "loop" => Opt::Loop,
            "scroll" => Opt::Scroll,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Opt::Anim => "anim",
            Opt::Hover => "hover",
            Opt::Loop => "loop",
            Opt::Scroll => "scroll",
        }
    }

    pub fn words(self) -> &'static [&'static str] {
        match self {
            Opt::Anim => ANIM,
            Opt::Hover => HOVER,
            Opt::Loop => LOOP,
            Opt::Scroll => SCROLL,
        }
    }

    /// Can a `keyframes(...)` word be used here?
    pub fn takes_custom(self) -> bool {
        matches!(self, Opt::Anim | Opt::Loop)
    }

    /// What a bare number means after the words.
    fn number_means(self) -> &'static str {
        match self {
            Opt::Anim | Opt::Loop => "a delay in seconds",
            Opt::Hover => "a strength (1 is normal)",
            Opt::Scroll => "a rate (0.2 is normal)",
        }
    }

    /// The prefix of the CSS variables the generated rules set.
    fn var(self) -> &'static str {
        match self {
            Opt::Anim => "--ma",
            Opt::Hover => "--mh",
            Opt::Loop => "--ml",
            Opt::Scroll => "--ms",
        }
    }
}

/// A motion option's value, read: `"rise slow 0.2"` is the word `rise`, the speed `slow` and the
/// number 0.2 (a delay for `anim:`).
#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub opt: Opt,
    /// The words, in order; at least one.
    pub words: Vec<String>,
    /// `fast` or `slow`.
    pub speed: Option<String>,
    /// An easing preset word, or `spring` with its two numbers.
    pub ease: Option<Ease>,
    /// The easing as written: `spring`, `spring 260 16`, `in-out`.
    pub ease_text: Option<String>,
    /// A bare number: a delay (`anim`, `loop`), a strength (`hover`) or a rate (`scroll`).
    pub number: Option<f64>,
    /// A duration in seconds, from `4s` or `600ms`.
    pub duration: Option<f64>,
}

impl Spec {
    /// Does this value need a rule of its own (a number, a duration, a spring of its own)?
    pub fn needs_rule(&self) -> bool {
        self.number.is_some()
            || self.duration.is_some()
            || matches!(&self.ease_text, Some(t) if t.contains(' '))
    }
}

/// What is wrong with a motion value. The caller reports it as E0374, with `nearest` as the hint.
#[derive(Clone, Debug, PartialEq)]
pub struct WordError {
    /// The part of the value that is wrong.
    pub token: String,
    pub message: String,
    /// The closest valid word, when there is one.
    pub nearest: Option<String>,
}

impl WordError {
    fn new(token: &str, message: String, nearest: Option<String>) -> WordError {
        WordError { token: token.to_string(), message, nearest }
    }

    /// The hint for the diagnostic.
    pub fn hint(&self) -> Option<String> {
        self.nearest.as_ref().map(|n| format!("did you mean `{n}`?"))
    }
}

/// Every word valid in some option, with its option.
fn all_words() -> impl Iterator<Item = (Opt, &'static str)> {
    Opt::ALL.into_iter().flat_map(|o| o.words().iter().map(move |w| (o, *w)))
}

/// Edits from `a` to `b`, where swapping two neighbors is one edit (`lfit` is one from `lift`).
fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=b.len() {
        d[0][j] = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

/// The closest candidate within a small edit distance.
fn nearest<'a>(token: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let allowed = (token.chars().count() / 3).clamp(1, 2);
    candidates
        .into_iter()
        .map(|c| (distance(token, c), c))
        .filter(|(d, _)| *d > 0 && *d <= allowed)
        .min_by_key(|(d, _)| *d)
        .map(|(_, c)| c.to_string())
}

/// A plain number: `0.2`, `12`, `-0.3`.
fn number(t: &str) -> Option<f64> {
    let ok = !t.is_empty()
        && t.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '-')
        && t.chars().any(|c| c.is_ascii_digit());
    ok.then(|| t.parse::<f64>().ok()).flatten().filter(|n| n.is_finite())
}

/// A duration: `4s`, `0.6s`, `600ms`, in seconds.
fn duration(t: &str) -> Option<f64> {
    if let Some(ms) = t.strip_suffix("ms") {
        return number(ms).map(|n| n / 1000.0);
    }
    number(t.strip_suffix('s')?)
}

/// Reads a motion option's value. `custom` are the names defined with `keyframes(...)`.
pub fn parse(opt: Opt, value: &str, custom: &[&str]) -> Result<Spec, WordError> {
    let toks: Vec<&str> = value.split_whitespace().collect();
    let mut spec = Spec { opt, words: Vec::new(), speed: None, ease: None, ease_text: None, number: None, duration: None };
    let name = opt.name();
    if toks.is_empty() {
        let msg = format!("`{name}:` is empty; give a word such as `{}`, or `none` for no motion", opt.words()[0]);
        return Err(WordError::new(value, msg, None));
    }
    let mut i = 0;
    while i < toks.len() {
        let t = toks[i];
        i += 1;
        if opt.words().contains(&t) || (opt.takes_custom() && custom.contains(&t)) {
            if spec.words.iter().any(|w| w == t) {
                return Err(WordError::new(t, format!("`{t}` is written twice in `{name}: \"{value}\"`"), None));
            }
            spec.words.push(t.to_string());
        } else if SPEEDS.contains(&t) {
            if let Some(s) = &spec.speed {
                return Err(WordError::new(t, format!("`{name}: \"{value}\"` has two speeds, `{s}` and `{t}`"), None));
            }
            spec.speed = Some(t.to_string());
        } else if let Some(e) = ease::preset(t) {
            if let Some(old) = &spec.ease_text {
                return Err(WordError::new(t, format!("`{name}: \"{value}\"` has two easings, `{old}` and `{t}`"), None));
            }
            // `spring 260 16`: stiffness and damping
            if t == "spring" && toks.get(i).and_then(|n| number(n)).is_some() {
                let (k, d) = (number(toks[i]).unwrap(), toks.get(i + 1).and_then(|n| number(n)));
                let Some(d) = d else {
                    let msg = format!(
                        "`spring` takes two numbers, the stiffness and the damping (`spring 260 16`), in `{name}: \"{value}\"`"
                    );
                    return Err(WordError::new(t, msg, None));
                };
                let s = Spring::new(k, d);
                s.check().map_err(|why| WordError::new(t, why, None))?;
                spec.ease = Some(Ease::Spring(s));
                spec.ease_text = Some(format!("spring {} {}", toks[i], toks[i + 1]));
                i += 2;
            } else {
                spec.ease = Some(e);
                spec.ease_text = Some(t.to_string());
            }
        } else if let Some(s) = duration(t) {
            if spec.duration.is_some() {
                return Err(WordError::new(t, format!("`{name}: \"{value}\"` has two durations"), None));
            }
            if !(0.05..=60.0).contains(&s) {
                return Err(WordError::new(t, format!("a duration is from 50ms to 60s (found `{t}`)"), None));
            }
            spec.duration = Some(s);
        } else if let Some(n) = number(t) {
            if spec.number.is_some() {
                let msg = format!(
                    "`{name}: \"{value}\"` has two numbers; after the words a number is {}",
                    opt.number_means()
                );
                return Err(WordError::new(t, msg, None));
            }
            let range = match opt {
                Opt::Anim | Opt::Loop => 0.0..=30.0,
                Opt::Hover => 0.0..=5.0,
                Opt::Scroll => -2.0..=2.0,
            };
            if !range.contains(&n) {
                let msg = format!(
                    "in `{name}:` a number is {}, from {} to {} (found `{t}`)",
                    opt.number_means(),
                    range.start(),
                    range.end()
                );
                return Err(WordError::new(t, msg, None));
            }
            spec.number = Some(n);
        } else {
            return Err(unknown(opt, t, value, custom));
        }
    }
    if spec.words.is_empty() {
        let msg = format!("`{name}: \"{value}\"` has no word, only how it moves; add one such as `{}`", opt.words()[0]);
        return Err(WordError::new(value, msg, None));
    }
    if spec.words.len() > 1 && spec.words.iter().any(|w| w == "none") {
        return Err(WordError::new("none", format!("`none` stands alone: `{name}: \"none\"`"), None));
    }
    Ok(spec)
}

/// The error for a word that is not valid here, with the nearest one.
fn unknown(opt: Opt, t: &str, value: &str, custom: &[&str]) -> WordError {
    let name = opt.name();
    if let Some((other, _)) = all_words().find(|(o, w)| *w == t && *o != opt) {
        let msg = format!("`{t}` is a word of `{}:`, not of `{name}:`", other.name());
        let near = nearest(t, opt.words().iter().copied());
        return WordError::new(t, msg, near);
    }
    if !opt.takes_custom() && custom.contains(&t) {
        let msg = format!("`{t}` is made with `keyframes`, which works in `anim:` and `loop:`, not in `{name}:`");
        return WordError::new(t, msg, None);
    }
    let mut cands: Vec<&str> = opt.words().to_vec();
    cands.extend(SPEEDS);
    cands.extend(ease::PRESETS);
    if opt.takes_custom() {
        cands.extend(custom);
    }
    let near = nearest(t, cands.iter().copied());
    let msg = if value.trim() == t {
        format!("`{t}` is not a word of `{name}:`")
    } else {
        format!("`{t}` in `{name}: \"{value}\"` is not a word of `{name}:`")
    };
    WordError::new(t, msg, near)
}

// ---------------------------------------------------------------------------------------------
// keyframes(name, body)

/// The properties a custom keyframe may animate silently: the compositor moves them without
/// layout or paint.
const COMPOSITOR: &[&str] =
    &["transform", "translate", "rotate", "scale", "opacity", "filter", "backdrop-filter", "clip-path", "background-position"];
/// Properties that repaint but do not move anything else: allowed silently.
const PAINT: &[&str] = &[
    "color",
    "background",
    "background-color",
    "background-image",
    "background-size",
    "box-shadow",
    "text-shadow",
    "border-color",
    "border-radius",
    "outline-color",
    "outline-offset",
    "fill",
    "stroke",
    "stroke-dashoffset",
    "stroke-dasharray",
    "stroke-width",
    "visibility",
    "mask-position",
    "mask-size",
    "transform-origin",
    "offset-distance",
    "z-index",
];
/// Properties that cause layout: allowed with a warning (E0375).
const LAYOUT: &[&str] = &[
    "width",
    "height",
    "min-width",
    "min-height",
    "max-width",
    "max-height",
    "top",
    "right",
    "bottom",
    "left",
    "inset",
    "margin",
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "padding",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "border-width",
    "font-size",
    "font-weight",
    "line-height",
    "letter-spacing",
    "word-spacing",
    "gap",
    "flex-basis",
    "flex-grow",
    "grid-template-columns",
    "grid-template-rows",
];

/// A checked `keyframes(name, body)`.
#[derive(Clone, Debug, PartialEq)]
pub struct Keyframes {
    pub name: String,
    /// Each block: its offsets in percent, then its declarations (property, value).
    pub frames: Vec<(Vec<f64>, Vec<(String, String)>)>,
    /// The properties that cause layout, in the order first used (E0375).
    pub layout: Vec<String>,
}

impl Keyframes {
    /// The CSS name of the animation: `m-k-sway`.
    pub fn css_name(&self) -> String {
        format!("m-k-{}", self.name)
    }

    /// `@keyframes m-k-sway{0%,100%{rotate:-3deg}50%{rotate:3deg}}`
    pub fn css(&self) -> String {
        let mut s = format!("@keyframes {}{{", self.css_name());
        for (offs, decls) in &self.frames {
            let o: Vec<String> = offs.iter().map(|p| format!("{}%", ease::num(*p, 3))).collect();
            let d: Vec<String> = decls.iter().map(|(p, v)| format!("{p}:{v}")).collect();
            s.push_str(&format!("{}{{{}}}", o.join(","), d.join(";")));
        }
        s.push('}');
        s
    }

    /// The warning for animating layout properties, if there are any (E0375).
    pub fn layout_warning(&self) -> Option<(String, String)> {
        if self.layout.is_empty() {
            return None;
        }
        let props: Vec<String> = self.layout.iter().map(|p| format!("`{p}`")).collect();
        let (s, them) = if props.len() == 1 { ("", "it") } else { ("s", "them") };
        let msg = format!(
            "the keyframes `{}` animate the layout propert{} {}: the page is laid out again on every frame",
            self.name,
            if s.is_empty() { "y" } else { "ies" },
            props.join(", ")
        );
        let hint = format!(
            "move {them} with `translate`, `scale` or `clip-path` instead, which the compositor animates without layout"
        );
        Some((msg, hint))
    }
}

/// What is wrong with a `keyframes(name, body)` call. The caller reports it as an error
/// (E0388 in `docs/WEB.md`).
#[derive(Clone, Debug, PartialEq)]
pub struct KeyframesError {
    pub message: String,
    pub hint: Option<String>,
}

fn kf_err(message: String, hint: Option<String>) -> KeyframesError {
    KeyframesError { message, hint }
}

/// Is `name` usable as the name of a new word?
fn check_name(name: &str) -> Result<(), KeyframesError> {
    let ok = name.len() <= 40
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !name.ends_with('-')
        && !name.contains("--");
    if !ok {
        let msg = format!("`{name}` cannot name a motion word: use lowercase letters, digits and single `-`, starting with a letter");
        return Err(kf_err(msg, Some("for example `keyframes(\"sway\", ...)`".to_string())));
    }
    if let Some((o, _)) = all_words().find(|(_, w)| *w == name) {
        let msg = format!("`{name}` is already a word of `{}:`; a keyframes word needs a new name", o.name());
        return Err(kf_err(msg, Some(format!("for example `my-{name}`"))));
    }
    if SPEEDS.contains(&name) || ease::PRESETS.contains(&name) || name == "spring" {
        let msg = format!("`{name}` is a speed or an easing, not a free name for a motion word");
        return Err(kf_err(msg, None));
    }
    Ok(())
}

/// Checks `keyframes(name, body)`: the name is a new word, the body is a CSS keyframes body
/// (`0%, 100% { rotate: -3deg } 50% { rotate: 3deg }`, also `from` and `to`) with known
/// properties and plain values. Layout properties are allowed but listed in `layout` (E0375).
pub fn check_keyframes(name: &str, body: &str) -> Result<Keyframes, KeyframesError> {
    check_name(name)?;
    let mut frames = Vec::new();
    let mut layout: Vec<String> = Vec::new();
    let mut rest = body.trim();
    if rest.is_empty() {
        return Err(kf_err(
            format!("the keyframes `{name}` have no body"),
            Some("for example `\"0%, 100% { rotate: -3deg } 50% { rotate: 3deg }\"`".to_string()),
        ));
    }
    while !rest.is_empty() {
        let Some(open) = rest.find('{') else {
            return Err(kf_err(format!("in the keyframes `{name}`, `{}` is not followed by `{{ ... }}`", short(rest)), None));
        };
        let Some(close) = rest[open..].find('}').map(|c| c + open) else {
            return Err(kf_err(format!("in the keyframes `{name}`, a `{{` is not closed"), None));
        };
        let sel = rest[..open].trim();
        let inner = &rest[open + 1..close];
        if inner.contains('{') {
            return Err(kf_err(format!("in the keyframes `{name}`, a `{{` is not closed before the next one"), None));
        }
        let mut offs = Vec::new();
        for part in sel.split(',') {
            let p = part.trim();
            let v = match p {
                "from" => Some(0.0),
                "to" => Some(100.0),
                _ => p.strip_suffix('%').and_then(number).filter(|v| (0.0..=100.0).contains(v)),
            };
            match v {
                Some(v) => offs.push(v),
                None => {
                    let msg = if p.is_empty() {
                        format!("in the keyframes `{name}`, a block has no offset before `{{`")
                    } else {
                        format!("in the keyframes `{name}`, `{p}` is not an offset")
                    };
                    let hint = "an offset is `from`, `to` or a percentage from `0%` to `100%`; several are separated by commas";
                    return Err(kf_err(msg, Some(hint.to_string())));
                }
            }
        }
        let mut decls = Vec::new();
        for d in inner.split(';') {
            let d = d.trim();
            if d.is_empty() {
                continue;
            }
            let Some((p, v)) = d.split_once(':') else {
                return Err(kf_err(format!("in the keyframes `{name}`, `{d}` is not `property: value`"), None));
            };
            let (p, v) = (p.trim().to_ascii_lowercase(), v.split_whitespace().collect::<Vec<_>>().join(" "));
            check_property(name, &p)?;
            check_value(name, &p, &v)?;
            if LAYOUT.contains(&p.as_str()) && !layout.contains(&p) {
                layout.push(p.clone());
            }
            decls.push((p, v));
        }
        if decls.is_empty() {
            return Err(kf_err(format!("in the keyframes `{name}`, the block `{sel}` is empty"), None));
        }
        frames.push((offs, decls));
        rest = rest[close + 1..].trim_start();
    }
    Ok(Keyframes { name: name.to_string(), frames, layout })
}

fn short(s: &str) -> String {
    let t: String = s.chars().take(24).collect();
    if t.len() < s.len() {
        format!("{t}...")
    } else {
        t
    }
}

fn check_property(name: &str, p: &str) -> Result<(), KeyframesError> {
    let base = p.strip_prefix("-webkit-").unwrap_or(p);
    let custom = p.starts_with("--") && p.len() > 2 && p[2..].chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if custom || [COMPOSITOR, PAINT, LAYOUT].iter().any(|l| l.contains(&base)) {
        return Ok(());
    }
    let all = COMPOSITOR.iter().chain(PAINT).chain(LAYOUT).copied();
    let near = nearest(p, all);
    let msg = format!("in the keyframes `{name}`, `{p}` is not a property keyframes can animate");
    let hint = near.map(|n| format!("did you mean `{n}`?")).or_else(|| {
        Some("the usual ones are `transform`, `translate`, `rotate`, `scale`, `opacity` and `filter`".to_string())
    });
    Err(kf_err(msg, hint))
}

/// A value is plain text: numbers, units, names, functions; nothing that could end the CSS
/// (`{`, `}`, `;`, `<`), load a file (`url(`), or escape (`\`, quotes, `@`, `!important`).
fn check_value(name: &str, p: &str, v: &str) -> Result<(), KeyframesError> {
    let bad = v.is_empty()
        || v.chars().any(|c| matches!(c, '{' | '}' | ';' | '<' | '>' | '\\' | '"' | '\'' | '@' | '!' | '`'))
        || v.to_ascii_lowercase().contains("url(")
        || v.to_ascii_lowercase().contains("expression(");
    let mut depth = 0i32;
    for c in v.chars() {
        depth += match c {
            '(' => 1,
            ')' => -1,
            _ => 0,
        };
        if depth < 0 {
            break;
        }
    }
    if bad || depth != 0 {
        let msg = if v.is_empty() {
            format!("in the keyframes `{name}`, `{p}` has no value")
        } else {
            format!("in the keyframes `{name}`, the value of `{p}` is not a plain CSS value: `{v}`")
        };
        return Err(kf_err(msg, Some("a value is numbers, units, names and functions like `rotate(3deg)`; no quotes, `url()` or `!important`".to_string())));
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Checking a program before it runs

/// What a [`lint`] found; the caller gives the codes in `docs/WEB.md` ("Motion build contract").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IssueKind {
    /// An unknown word in a motion option, `transition:`, `motion:` or a bad `at:`/`morph:` (E0374).
    UnknownWord,
    /// A `keyframes` body that animates a layout property (E0375, a warning).
    LayoutProperty,
    /// A `keyframes` call whose name or body is wrong (E0388).
    BadKeyframes,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Issue {
    pub kind: IssueKind,
    pub span: Span,
    pub message: String,
    pub hint: Option<String>,
}

/// Checks the motion options and `keyframes(...)` calls of a program whose values are string
/// literals, from its tokens. Computed values are checked by the build ([`parse`]).
///
/// It looks at `anim:`, `hover:`, `loop:`, `scroll:`, `transition:`, `motion:`, `morph:` and
/// `at:` arguments, and at `keyframes("name", "body")` (or `_keyframes`) calls.
pub fn lint(toks: &[Token]) -> Vec<Issue> {
    let tok = |i: usize| toks.get(i).map(|t| &t.tok);
    let lit = |i: usize| match tok(i) {
        Some(Tok::Str(s)) => Some(s.as_str()),
        _ => None,
    };
    let mut issues = Vec::new();
    // first the keyframes, so their names are words everywhere in the program
    let mut custom: Vec<String> = Vec::new();
    for i in 0..toks.len() {
        let is_call = matches!(tok(i), Some(Tok::Ident(f)) if f == "keyframes" || f == "_keyframes")
            && tok(i + 1) == Some(&Tok::LParen)
            && tok(i + 3) == Some(&Tok::Comma)
            && tok(i + 5) == Some(&Tok::RParen);
        let (Some(name), Some(body)) = (lit(i + 2), lit(i + 4)) else { continue };
        if !is_call {
            continue;
        }
        match check_keyframes(name, body) {
            Ok(kf) => {
                if let Some((message, hint)) = kf.layout_warning() {
                    issues.push(Issue { kind: IssueKind::LayoutProperty, span: toks[i + 4].span, message, hint: Some(hint) });
                }
                custom.push(kf.name);
            }
            Err(e) => {
                // a bad name is reported at the name, a bad body at the body
                let at = if check_name(name).is_err() { i + 2 } else { i + 4 };
                issues.push(Issue { kind: IssueKind::BadKeyframes, span: toks[at].span, message: e.message, hint: e.hint });
            }
        }
    }
    let custom: Vec<&str> = custom.iter().map(|s| s.as_str()).collect();
    for i in 1..toks.len() {
        let Some(Tok::Ident(label)) = tok(i) else { continue };
        let in_args = matches!(tok(i - 1), Some(Tok::LParen | Tok::Comma));
        if !in_args || tok(i + 1) != Some(&Tok::Colon) {
            continue;
        }
        let span = toks.get(i + 2).map(|t| t.span).unwrap_or(toks[i].span);
        let mut push = |message: String, hint: Option<String>| {
            issues.push(Issue { kind: IssueKind::UnknownWord, span, message, hint });
        };
        if let Some(opt) = Opt::from_name(label).filter(|_| !label.starts_with("data-")) {
            let Some(v) = lit(i + 2) else { continue };
            if let Err(e) = parse(opt, v, &custom) {
                let hint = e.hint();
                push(e.message, hint);
            }
        } else if label == "transition" || label == "motion" {
            let Some(v) = lit(i + 2) else { continue };
            let list = if label == "transition" { TRANSITIONS } else { LEVELS };
            if !list.contains(&v) {
                let words: Vec<String> = list.iter().map(|w| format!("`{w}`")).collect();
                let near = nearest(v, list.iter().copied()).map(|n| format!("did you mean `{n}`?"));
                push(format!("`{label}: \"{v}\"` is not one of {}", words.join(", ")), near);
            }
        } else if label == "morph" {
            let Some(v) = lit(i + 2) else { continue };
            if morph_name(v).is_none() {
                push(format!("`morph: \"{v}\"` is not a name: use letters, digits and `-`, starting with a letter"), None);
            }
        } else if label == "at" {
            let n = match (tok(i + 2), tok(i + 3)) {
                (Some(Tok::Int(n)), _) => Some(*n as f64),
                (Some(Tok::Float(f)), _) => Some(*f),
                (Some(Tok::Minus), Some(Tok::Int(_) | Tok::Float(_))) => Some(-1.0),
                _ => None,
            };
            if let Some(n) = n.filter(|n| !(0.0..=30.0).contains(n)) {
                push(format!("`at: {}` is outside 0 to 30 seconds after the parent's start", ease::num(n, 3)), None);
            }
        }
    }
    issues
}

/// Checks the source of a program: [`lint`] on its tokens.
pub fn lint_source(src: &str) -> Vec<Issue> {
    let (toks, _) = crate::lexer::lex(src);
    lint(&toks)
}

/// The `view-transition-name` for a `morph:` value.
fn morph_name(v: &str) -> Option<String> {
    let ok = !v.is_empty()
        && v.len() <= 60
        && v.starts_with(|c: char| c.is_ascii_alphabetic())
        && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    ok.then(|| format!("m-{}", v.to_ascii_lowercase()))
}

// ---------------------------------------------------------------------------------------------
// Assembling the CSS and JS for a site

/// The parts of a file: (name, text), in file order. A part starts with `/*! motion: name */`.
fn parts(src: &'static str) -> Vec<(&'static str, &'static str)> {
    let mut out = Vec::new();
    let marker = "/*! motion: ";
    let mut rest = src;
    while let Some(start) = rest.find(marker) {
        let after = &rest[start + marker.len()..];
        let end = after.find(" */").expect("a motion part marker ends with ` */`");
        let name = &after[..end];
        let body_start = &after[end + 3..];
        let next = body_start.find(marker).unwrap_or(body_start.len());
        out.push((name, &body_start[..next]));
        rest = &body_start[next..];
    }
    out
}

/// Every attribute value of the motion attributes in some HTML: (`anim`, `rise slow 0.2`).
/// The build can collect what the pages use with it.
pub fn scan_html(html: &str, out: &mut Vec<(String, String)>) {
    const NAMES: &[&str] = &["anim", "hover", "loop", "scroll", "at", "motion", "morph"];
    let mut rest = html;
    while let Some(p) = rest.find("data-") {
        let after = &rest[p + 5..];
        rest = after;
        let Some(name) = NAMES.iter().find(|n| after.starts_with(**n) && after[n.len()..].starts_with('=')) else { continue };
        let v = &after[name.len() + 1..];
        let value = match v.chars().next() {
            Some(q @ ('"' | '\'')) => v[1..].split(q).next().unwrap_or(""),
            _ => v.split(|c: char| c.is_whitespace() || c == '>').next().unwrap_or(""),
        };
        let pair = (name.to_string(), value.to_string());
        if !out.contains(&pair) {
            out.push(pair);
        }
    }
}

/// A value safe inside `[data-x="..."]`: motion values are words, numbers and spaces.
fn attr_safe(v: &str) -> bool {
    v.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '-' | '_'))
}

/// What a site uses, read from the (attribute, value) pairs.
struct Used {
    specs: Vec<(Spec, String)>,
    at: Vec<(String, f64)>,
    morph: Vec<(String, String)>,
    level: Option<String>,
    transition: Option<String>,
}

fn read(used: &[(&str, &str)], custom: &[&str]) -> Used {
    let mut u = Used { specs: Vec::new(), at: Vec::new(), morph: Vec::new(), level: None, transition: None };
    for &(attr, value) in used {
        let attr = attr.strip_prefix("data-").unwrap_or(attr);
        let value = value.trim();
        if let Some(opt) = Opt::from_name(attr) {
            if let Ok(s) = parse(opt, value, custom) {
                if attr_safe(value) && !u.specs.iter().any(|(o, v)| o.opt == opt && v == value) {
                    u.specs.push((s, value.to_string()));
                }
            }
        } else if attr == "at" {
            if let Some(n) = number(value).filter(|n| (0.0..=30.0).contains(n)) {
                if attr_safe(value) && !u.at.iter().any(|(v, _)| v == value) {
                    u.at.push((value.to_string(), n));
                }
            }
        } else if attr == "morph" {
            if let Some(n) = morph_name(value) {
                if !u.morph.iter().any(|(v, _)| v == value) {
                    u.morph.push((value.to_string(), n));
                }
            }
        } else if attr == "motion" && LEVELS.contains(&value) {
            u.level = Some(value.to_string());
        } else if attr == "transition" && TRANSITIONS.contains(&value) {
            u.transition = Some(value.to_string());
        }
    }
    u
}

/// The rule of one value with numbers: `[data-anim="rise slow 0.2"]{--ma-d:.2s}`.
fn value_rule(spec: &Spec, value: &str) -> String {
    let o = spec.opt;
    let sel = format!("[data-{}=\"{value}\"]", o.name());
    let mut decls: Vec<String> = Vec::new();
    if let Some(n) = spec.number {
        decls.push(match o {
            Opt::Anim => format!("--ma-d:{}s", ease::num(n, 3)),
            Opt::Loop => format!("--ml-d:{}s", ease::num(n, 3)),
            Opt::Hover => format!("--mh-k:{}", ease::num(n, 3)),
            Opt::Scroll => format!("--ms-r:{}", ease::num(n, 3)),
        });
    }
    let dur_var = match o {
        Opt::Anim => "--m-dur",
        Opt::Hover => "--mh-dur",
        Opt::Loop => "--ml-dur",
        Opt::Scroll => "",
    };
    let mut supports = String::new();
    if let (Some(Ease::Spring(s)), Some(t)) = (spec.ease, &spec.ease_text) {
        if t.contains(' ') {
            let c = s.curve();
            decls.push(format!("{}-e:{}", o.var(), c.bezier_css()));
            if o != Opt::Loop && !dur_var.is_empty() {
                decls.push(format!("{dur_var}:{}s", ease::num(c.duration, 2)));
            }
            supports = format!("@supports (transition-timing-function:linear(0,1)){{{sel}{{{}-e:{}}}}}\n", o.var(), c.linear());
        }
    }
    if let Some(d) = spec.duration.filter(|_| !dur_var.is_empty()) {
        decls.push(format!("{dur_var}:{}s", ease::num(d, 3)));
    }
    if decls.is_empty() {
        return String::new();
    }
    format!("{sel}{{{}}}\n{supports}", decls.join(";"))
}

/// The rules of a `keyframes` word for the options it is used in.
fn custom_rules(kf: &Keyframes, anim: bool, lp: bool) -> String {
    let (w, k) = (&kf.name, kf.css_name());
    let mut s = format!("{}\n", kf.css());
    if anim {
        s.push_str(&format!("[data-anim~={w}]{{--m-o:1}}\n"));
        s.push_str(&format!(
            "@media (scripting:enabled) and (prefers-reduced-motion:no-preference){{:root:not([data-motion=off]) [data-anim~={w}]:not(.m-in){{--m-a1:{k} 1s paused both}}}}\n"
        ));
        s.push_str(&format!(
            "[data-anim~={w}].m-in{{--m-a1:{k} calc(var(--m-dur,1s)*var(--ma-t)*var(--m-k)) var(--ma-e) calc(var(--m-at) + var(--ma-d) + var(--m-i,0)*var(--m-step,0s)) both}}\n"
        ));
    }
    if lp {
        s.push_str(&format!(
            "[data-loop~={w}]{{--m-a2:{k} calc(var(--ml-dur,2s)*var(--ml-t)) var(--ml-e,ease-in-out) var(--ml-d) infinite}}\n"
        ));
    }
    s
}

/// Removes comments (and so the part markers) and blank lines; trims each line.
fn strip_css(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(p) = rest.find("/*") {
        out.push_str(&rest[..p]);
        rest = rest[p..].find("*/").map(|e| &rest[p + e + 2..]).unwrap_or("");
    }
    out.push_str(rest);
    out.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n") + "\n"
}

/// Removes comments and indentation from the JS (it keeps its line breaks).
fn strip_js(s: &str) -> String {
    let mut out = Vec::new();
    let mut in_block = false;
    for line in s.lines() {
        let l = line.trim();
        if in_block {
            in_block = !l.contains("*/");
            continue;
        }
        if l.starts_with("/*") {
            in_block = !l.contains("*/");
            continue;
        }
        if l.is_empty() || l.starts_with("//") {
            continue;
        }
        out.push(l);
    }
    squeeze_js(&(out.join("\n") + "\n"))
}

/// Drops the spaces JS does not need: a space stays only between two letters or digits. Strings,
/// template literals and regular expressions are copied as they are. (Line breaks stay, so no
/// statement depends on how the next line starts.)
fn squeeze_js(s: &str) -> String {
    let c: Vec<char> = s.chars().collect();
    let word = |ch: char| ch.is_alphanumeric() || ch == '_' || ch == '$';
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        match ch {
            '\'' | '"' | '`' => {
                out.push(ch);
                i += 1;
                while i < c.len() && c[i] != ch {
                    if c[i] == '\\' {
                        out.push(c[i]);
                        i += 1;
                    }
                    out.push(c[i]);
                    i += 1;
                }
                out.push(ch);
            }
            // a `/` after an operator or an opening bracket starts a regular expression
            '/' if out.chars().last().is_none_or(|p| "(,=:[!&|?{};\n".contains(p)) => {
                let mut class = false;
                out.push(ch);
                i += 1;
                while i < c.len() && (class || c[i] != '/') {
                    match c[i] {
                        '\\' => {
                            out.push(c[i]);
                            i += 1;
                        }
                        '[' => class = true,
                        ']' => class = false,
                        _ => {}
                    }
                    out.push(c[i]);
                    i += 1;
                }
                out.push('/');
            }
            ' ' | '\t' => {
                let next = c.get(i + 1).copied();
                if out.chars().last().is_some_and(word) && next.is_some_and(word) {
                    out.push(' ');
                }
            }
            _ => out.push(ch),
        }
        i += 1;
    }
    out
}

/// The motion CSS for a site: the base, the parts of the words its pages use (in `used`, pairs
/// of attribute and value as the renderer wrote them, see [`scan_html`]; `transition` is
/// `site(transition:)`), the `keyframes` words, and a rule for each value with numbers.
/// Empty when nothing moves. Values that do not parse are left out (the build reports them).
pub fn motion_css(used: &[(&str, &str)], custom: &[Keyframes]) -> String {
    let names: Vec<&str> = custom.iter().map(|k| k.name.as_str()).collect();
    let u = read(used, &names);
    let off = u.level.as_deref() == Some("off");
    let transition = u.transition.as_deref().filter(|t| *t != "none" && !off);
    if u.specs.is_empty() && u.morph.is_empty() && transition.is_none() {
        return String::new();
    }
    let mut want: Vec<String> = vec!["base".into(), "mods".into()];
    let mut custom_use: Vec<(&Keyframes, bool, bool)> = custom.iter().map(|k| (k, false, false)).collect();
    for (s, _) in &u.specs {
        if s.opt == Opt::Anim {
            want.push("reveal".into());
        }
        for w in &s.words {
            if let Some(c) = custom_use.iter_mut().find(|(k, _, _)| &k.name == w) {
                if s.opt == Opt::Anim {
                    c.1 = true;
                } else {
                    c.2 = true;
                }
            } else {
                want.push(format!("{} {w}", s.opt.name()));
            }
        }
    }
    // the springs of the easing words a value names (and `pop`, which bounces)
    for (s, _) in &u.specs {
        let named = s.ease_text.as_deref().filter(|t| !t.contains(' '));
        for e in ["spring", "bounce", "snappy"] {
            if named == Some(e) || (e == "bounce" && s.opt == Opt::Anim && s.words.iter().any(|w| w == "pop")) {
                want.push(format!("ease {e}"));
            }
        }
    }
    if let Some(t) = transition {
        want.push("transition".into());
        want.push(format!("transition {t}"));
    }
    let mut css: String = parts(CSS).into_iter().filter(|(n, _)| want.iter().any(|w| w == n)).map(|(_, b)| b).collect();
    for (k, a, l) in custom_use.into_iter().filter(|(_, a, l)| *a || *l) {
        css.push_str(&custom_rules(k, a, l));
    }
    for (s, v) in &u.specs {
        if s.needs_rule() {
            css.push_str(&value_rule(s, v));
        }
    }
    for (v, n) in &u.at {
        css.push_str(&format!("[data-at=\"{v}\"]{{--m-at:{}s}}\n", ease::num(*n, 3)));
    }
    for (v, n) in &u.morph {
        css.push_str(&format!("[data-morph=\"{v}\"]{{view-transition-name:{n}}}\n"));
    }
    strip_css(&css)
}

/// The motion JS for a site: the core and the parts its words need. Empty when no word needs a
/// script (hover, loop and scroll words other than `tilt`, `magnetic`, `marquee` and the
/// scroll fallback are CSS only).
pub fn motion_js(used: &[(&str, &str)]) -> String {
    let mut want: Vec<&str> = Vec::new();
    for &(attr, value) in used {
        let attr = attr.strip_prefix("data-").unwrap_or(attr);
        let words: Vec<&str> = value.split_whitespace().collect();
        let has = |w: &str| words.contains(&w);
        match attr {
            // any anim value, also a keyframes word, needs the reveal
            "anim" | "at" => {
                want.push("core");
                if has("words") || has("letters") {
                    want.push("words");
                }
                if has("count") {
                    want.push("count");
                }
                if has("draw") {
                    want.push("draw");
                }
            }
            "hover" => {
                if has("tilt") {
                    want.push("tilt");
                }
                if has("magnetic") {
                    want.push("magnetic");
                }
            }
            "loop" if has("marquee") => want.push("marquee"),
            "scroll" if SCROLL.iter().any(|w| has(w)) => want.push("scroll"),
            _ => {}
        }
    }
    if want.is_empty() {
        return String::new();
    }
    want.push("core");
    want.push("end");
    let js: String = parts(JS).into_iter().filter(|(n, _)| want.contains(n)).map(|(_, b)| b).collect();
    strip_js(&js)
}

/// The springs behind the easing words, as `:root` variables: the part `ease <name>` of
/// `motion.css` holds this text (a test keeps them equal), so the file works as it is.
pub fn easing_part(name: &str) -> String {
    let (k, d) = match name {
        "spring" => ease::SPRING,
        "bounce" => ease::BOUNCE,
        _ => ease::SNAPPY,
    };
    let c = Spring::new(k, d).curve();
    format!(
        ":root{{--m-{name}:{};--m-{name}-t:{}s}}
@supports (transition-timing-function:linear(0,1)){{:root{{--m-{name}:{}}}}}
",
        c.bezier_css(),
        ease::num(c.duration, 2),
        c.linear()
    )
}

/// Removes everything but the CSS (comments, the part marker, blank lines and indentation).
fn plain(part: &str) -> String {
    strip_css(part)
}

#[cfg(test)]
mod tests;
