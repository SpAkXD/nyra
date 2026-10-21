//! `use web` (docs/WEB.md): links the web library (`src/std/web.nyra` and its icons) into a
//! program and checks what can be checked before the program runs.
//!
//! - The library's functions are added as `web.name`, like any standard module's, and the
//!   program's calls of their bare names (`hero(...)`) are turned into calls of `web.hero`. A
//!   program function, struct or script variable with the name of a web function or type is
//!   E0372.
//! - The site lives in a script variable, `_web`, declared before the program's first statement.
//! - `on: Msg.X(..)` becomes the JSON text of the value (`web._on(m: Msg)`); a program with `on:`
//!   needs `enum Msg` and `fn update(m: Msg)` (E0373).
//! - Live regions: a function without parameters that returns a `Node` and reads a script
//!   variable that `update` changes is called wrapped in `web._live` (a `data-live` region the
//!   browser renders again). One with parameters is E0376; the code that runs in the browser may
//!   not use `fs`, `os` or `input` (E0384).
//! - Literal arguments are checked: icon names and option words (E0374), icon-only buttons
//!   without a label (E0377), `html(...)` and `css(...)` with a literal only (E0378), links with
//!   a forbidden scheme (E0381). Computed text is checked by the library when it renders, which
//!   prints an error record for the build.

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::ast::*;
use crate::diag::{suggest, Diag, Edit};

/// The library: the elements and kit components, then the generated icon set.
pub const SOURCE: &str = concat!(include_str!("std/web.nyra"), "\n", include_str!("std/web_icons.nyra"));

/// The script variable that holds the site (`WebConfig`).
pub const STATE: &str = "_web";

/// The types of the library a program may name.
pub const TYPES: &[&str] = &["Node", "WebConfig"];

fn library(src: &str) -> Program {
    let (toks, errs) = crate::lexer::lex(src);
    assert!(errs.is_empty(), "the web library does not lex: {errs:?}");
    let (prog, errs) = crate::parser::parse(toks);
    assert!(errs.is_empty(), "the web library does not parse: {errs:?}");
    prog
}

/// The public functions of the library and their parameter names, sorted by name.
#[cfg(test)]
fn functions() -> Vec<(String, Vec<String>)> {
    let mut v: Vec<(String, Vec<String>)> = library(SOURCE)
        .funcs
        .into_iter()
        .filter(|f| !f.name.starts_with('_'))
        .map(|f| (f.name, f.params.into_iter().map(|p| p.name).collect()))
        .collect();
    v.sort();
    v
}

/// The arms `"name" => "value"` of the `match` of the function `func` of the icon set.
fn icon_arms(func: &str) -> Vec<(&'static str, &'static str)> {
    let src: &'static str = include_str!("std/web_icons.nyra");
    let Some(start) = src.find(&format!("fn {func}(")) else { return Vec::new() };
    let body = &src[start..];
    let body = &body[..body.find("\n}").unwrap_or(body.len())];
    body.lines()
        .filter_map(|l| {
            let rest = l.trim_start().strip_prefix('"')?;
            let (name, value) = rest.split_once("\" => ")?;
            Some((name, value.trim().trim_matches('"')))
        })
        .collect()
}

/// The names of the icons (`:name:`, `icon: "name"`), sorted.
pub fn icons() -> Vec<String> {
    let mut v: Vec<String> = icon_arms("_icon_body").into_iter().chain(icon_arms("_brand_body")).map(|(n, _)| n.to_string()).collect();
    v.sort();
    v.dedup();
    v
}

/// The icon of the set that a name of another icon set stands for (`envelope`: `mail`): the
/// `|name=icon|` pairs of `_icon_like` in the icon set.
pub fn icon_like(name: &str) -> Option<&'static str> {
    let src: &'static str = include_str!("std/web_icons.nyra");
    let body = &src[src.find("fn _icon_like(")?..];
    let pairs = body.split_once("let pairs = \"")?.1.split('"').next()?;
    pairs.split('|').filter_map(|p| p.split_once('=')).find(|(n, _)| *n == name).map(|(_, v)| v)
}

/// Warning E0389 for an icon name that is not in the set (`text`: written `:name:` in a text).
/// The build draws the icon the name stands for (`icon_like`); else an icon option gets a plain
/// circle and a text keeps the name as it was written. The same message at compile time (a
/// literal) and while the site is built (a value from data), so it is given once.
pub fn icon_warning(name: &str, text: bool, span: Span) -> Diag {
    let shown = if text { format!(":{name}:") } else { name.to_string() };
    match icon_like(name) {
        Some(like) => {
            let what = if text { format!("`:{like}:`") } else { format!("`{like}`") };
            let d = Diag::new("E0389", format!("unknown icon `{shown}`: drawn as `{like}`"), span)
                .hint(format!("the Lucide name is {what}"));
            if text {
                d
            } else {
                d.fix(vec![Edit::replace(span, &format!("\"{name}\""), format!("\"{like}\""))])
            }
        }
        None => {
            let guesses: Vec<String> = icon_guesses(name).iter().map(|i| format!("`{i}`")).collect();
            let drawn = if text { "shown as it is written" } else { "drawn as a plain circle" };
            let hint = match guesses.len() {
                0 => "the icons are Lucide names, e.g. `star`, `check`, `heart`".to_string(),
                1 => format!("use {}", guesses[0]),
                n => format!("use {} or {}", guesses[..n - 1].join(", "), guesses[n - 1]),
            };
            Diag::new("E0389", format!("unknown icon `{shown}`: {drawn}"), span).hint(hint)
        }
    }
}

/// What common words mean as icons of the set: for an unknown icon name, the icons that say the
/// same (`siren`: `bell`, `triangle-alert`, ...). One line per meaning: the words, `:`, the icons.
const MEANINGS: &str = "\
alarm siren emergency urgent alert sos: bell triangle-alert phone-call
danger warning caution hazard error problem: triangle-alert circle-alert shield
ambulance hospital clinic medical medicine health healthcare: stethoscope activity heart
doctor nurse physician: stethoscope user activity
heart-pulse heartbeat pulse cardio: activity heart stethoscope
tooth teeth dental dentist implant braces orthodontics: smile sparkles stethoscope
pill pharmacy drug drugs prescription capsule: pill stethoscope syringe
vaccine injection needle vaccination: syringe stethoscope shield-check
clean cleaning hygiene whitening polish: sparkles droplets shield-check
fitness gym workout exercise weight weights muscle strength training barbell: dumbbell activity trophy
run running runner cardio-run marathon jog: activity timer trending-up
yoga meditation pilates stretch mindfulness: leaf sun heart
spa massage wellness relax relaxation sauna: leaf droplets sparkles
car auto vehicle automobile: car truck wrench
repair mechanic garage service servicing maintenance fix: wrench car settings
tire tyre tires tyres wheel wheels: car circle settings
engine motor gearbox: settings car zap
oil lubricant fluid: droplet fuel car
gauge speed speedometer dashboard performance meter: activity timer trending-up
battery charge charging electric electricity power energy plug: zap fuel sun
parking park: car map-pin building
square box-shape rectangle: layout-grid grid circle
circle-dot dot point bullet record: circle target circle-check
calculator calculate math accounting finance quote-price estimate: percent receipt chart-column
calendar-check booking book appointment reservation reserve schedule scheduled: calendar calendar-days clock
date event events day: calendar calendar-days clock
time hour hours opening open-hours: clock timer calendar
paint-bucket paint painting painter decor decorating: paint-roller paintbrush palette
room rooms bed bedroom sleep stay accommodation hostel motel inn lodging guesthouse: hotel house moon
guest guests visitor visitors: users user hand-heart
family kids children child parents: users baby heart
bath bathroom shower toilet: droplets droplet waves
pool swim swimming swimmer: waves droplets sun
beach sea ocean lake river water: waves droplet umbrella
hike hiking trail trek climbing climb: mountain compass tree-pine
nature garden gardening plant plants eco green outdoor: leaf sprout flower
forest tree woods park-trees: trees tree-pine leaf
weather forecast climate: cloud-sun sun cloud
rain rainy storm: umbrella cloud droplets
cold winter snow ice frozen freezer: snowflake thermometer cloud
hot heat warm temperature heating thermostat: thermometer flame sun
fire fireplace flame-fire bbq grill: flame utensils chef-hat
internet online wireless connection network: wifi globe server
television tv-screen screen display: tv monitor laptop
breakfast food meal restaurant dining eat kitchen lunch dinner: utensils coffee chef-hat
cook cooking chef cuisine: chef-hat utensils flame
bakery bread baker baking: croissant wheat cake
pastry dessert sweet sweets: cake-slice cookie donut
drink drinks bar cocktail cocktails pub: wine beer cup-soda
cafe tea espresso latte: coffee cup-soda croissant
fruit fruits: apple cherry carrot
vegetable vegetables vegan vegetarian organic veggie salad: carrot leaf sprout
pet pets animal animals puppy kitten: dog cat heart
vet veterinary veterinarian: dog stethoscope heart
money price prices pricing cost costs payment pay cash fee fees: banknote credit-card wallet
discount sale offer offers deal deals promo promotion coupon voucher: percent tag gift
shop shopping buy store cart basket: shopping-bag shopping-cart store
delivery shipping transport van courier: truck package map-pin
parcel box boxes: package truck gift
location address place where directions map-marker pin: map-pin map navigation
contact call telephone: phone mail message-circle
email letter newsletter inbox: mail send message-circle
message chat sms text comment comments: message-circle message-square send
support help question questions faq: circle-help message-circle info
about information details: info file-text circle-help
success done ok complete completed approved yes tick: circle-check check badge-check
close cancel remove delete no: x circle-x minus
security secure safe safety protect protection insurance guarantee warranty: shield-check shield lock
private password login account-lock privacy: lock key shield
access entry door keys: key lock house
award prize quality best certificate certified certification accredited: award medal badge-check
winner champion win victory first: trophy medal crown
premium vip luxury exclusive royal: crown gem sparkles
rating review reviews favorite favourite stars: star thumbs-up heart
like love loved: heart thumbs-up smile
team staff people community members crew employees: users user handshake
person profile account owner member customer client: user users smile
customers clients: users handshake star
partner partners partnership cooperation agreement: handshake users briefcase
care charity donate donation volunteer heart-handshake caring: hand-heart heart handshake
education school learn learning course courses training class classes lesson student teacher: graduation-cap book-open school
book books read reading library blog article news: book-open file-text bookmark
document documents file pdf contract form paperwork: file-text clipboard-list printer
list checklist tasks todo plan steps: list-checks clipboard-list check
work job jobs business career careers office services: briefcase building handshake
tool tools handyman plumbing plumber electrician: wrench hammer drill
construction builder building-site renovation build: hard-hat hammer construction
measure size ruler dimensions: ruler scale maximize-2
design creative art artist drawing: palette pen-tool paintbrush
photo photos photography picture pictures gallery camera-photo: camera image images
video videos film movie movies cinema: video film play
music song songs audio sound concert band: music headphones mic
podcast microphone speaker voice: mic headphones music
computer pc desktop: monitor laptop cpu
mobile app cellphone: smartphone phone wifi
developer programming software dev coding: code terminal cpu
hosting server data storage-data backup: server database cloud
cloud-storage upload-file: cloud-upload upload cloud
settings config configuration options preferences gear: settings sliders-horizontal wrench
chart stats statistics analytics growth graph report results: chart-column chart-line trending-up
goal aim focus mission: target trophy rocket
launch start fast quick speed-fast: rocket zap timer
idea innovation tip tips insight: lightbulb sparkles rocket
new magic special shine sparkle sparkling: sparkles star gem
gift present gifts: gift party-popper heart
party celebration birthday wedding anniversary: party-popper cake gift
travel trip flight airport vacation holiday holidays tour tours: plane map compass
rail railway: train-front map-pin ticket
bicycle cycling cyclist bikes: bike map-pin activity
boat ferry cruise sailing: ship waves anchor
camping camp campsite: tent tree-pine flame
chapel cathedral: church landmark building
museum history culture heritage castle monument: landmark history building-2
city town downtown urban: building-2 building map-pin
industry manufacturing plant-factory production: factory warehouse settings
storage stock inventory: warehouse package box
home house real-estate property apartment flat rent rental: house building key
farm farming agriculture harvest grain crops: wheat sprout leaf
recycling sustainable sustainability environment: recycle leaf sprout
wind-energy windy breeze: wind cloud leaf
sunny summer day-light: sun cloud-sun thermometer
night evening moon-night: moon star clock
translate translation international global world worldwide multilingual: languages globe flag
wheelchair disabled disability accessible: accessibility hand-heart user
infant newborn toddler: baby heart smile
country nation report: flag globe map
share social: share-2 link send
website web url homepage: globe link external-link
printing print: printer file-text image
testimonial testimonials quote-mark: quote message-square star
invoice bill bills order orders receipt-paper: receipt file-text credit-card
savings save bank budget invest investment: piggy-bank coins banknote
currency dollar euro-sign: dollar-sign euro coins
law legal justice lawyer attorney court: scale landmark file-text
haircut hair barber salon hairdresser: scissors sparkles smile
beauty cosmetics makeup nails: sparkles palette smile
label category categories tags: tag layers bookmark
layout dashboard-grid grid-view: layout-grid grid layers
menu-button hamburger navigation-menu: menu ellipsis layout-grid
next forward continue: arrow-right chevron-right move-right
back previous return: arrow-left chevron-left history
top up: arrow-up chevron-up trending-up
down bottom: arrow-down chevron-down chevrons-down
fullscreen enlarge zoom: expand maximize-2 zoom-in
add plus-sign more: plus circle-check sparkles
stopwatch countdown duration timer-clock: timer hourglass clock
past archive previous-version: history clock bookmark
";

/// The icons of the set that the unknown `name` most likely means: by meaning (`MEANINGS`), then by
/// spelling, then the icons that share a word with it (`circle-dot`: `circle`). At most three.
pub fn icon_guesses(name: &str) -> Vec<String> {
    let known = icons();
    let mut out: Vec<String> = Vec::new();
    let add = |i: &str, out: &mut Vec<String>| {
        if out.len() < 3 && i != name && known.iter().any(|k| k == i) && !out.iter().any(|o| o == i) {
            out.push(i.to_string());
        }
    };
    let parts: Vec<&str> = name.split('-').filter(|p| p.len() >= 3).collect();
    // the whole name first, then each of its words
    for word in std::iter::once(name).chain(parts.iter().copied()) {
        for line in MEANINGS.lines() {
            let Some((words, icons)) = line.split_once(':') else { continue };
            if words.split_whitespace().any(|w| w == word) {
                icons.split_whitespace().for_each(|i| add(i, &mut out));
            }
        }
    }
    let mut near: Vec<(usize, &String)> = known.iter().map(|i| (edits(name, i), i)).filter(|(d, _)| *d <= 2).collect();
    near.sort();
    near.iter().for_each(|(_, i)| add(i, &mut out));
    known.iter().filter(|i| i.split('-').any(|p| parts.contains(&p))).for_each(|i| add(i, &mut out));
    out
}

/// The option words the library knows, by parameter (and by function where they differ).
fn words(func: &str, param: &str) -> Option<&'static [&'static str]> {
    Some(match (func, param) {
        ("section", "bg") => &["accent", "alt", "dark", "ink", "primary", "white"],
        ("site", "style") => &["dark", "editorial", "sharp", "soft"],
        ("button", "style") => &["accent", "ghost", "light", "outline", "primary"],
        ("button", "size") => &["lg", "sm"],
        ("p", "size") => &["lg", "sm"],
        ("field", "type") => &["area", "check", "date", "email", "number", "password", "select", "tel", "text", "time", "url"],
        ("section", "align") => &["center", "left"],
        ("section", "width") => &["narrow", "wide"],
        ("stack" | "row", "align") => &["center", "end", "start", "between"],
        ("grid", "align") => &["center", "start"],
        ("hero", "height") => &["full", "half"],
        ("img", "fit") => &["contain", "cover"],
        _ => return None,
    })
}

/// True if `word` is one of the words of option `param:` of the web function `func` (a motion word,
/// an option word, a theme color): written without quotes (`type: tel`, `style: soft`, as on the
/// web card), it is that word, so the quotes are a certain fix.
pub fn option_word(func: &str, param: &str, word: &str) -> bool {
    if let Some(opt) = crate::web::motion::Opt::from_name(param) {
        return opt.words().contains(&word);
    }
    if matches!(param, "color" | "tone") {
        return matches!(word, "primary" | "accent" | "bg" | "alt" | "ink" | "muted" | "white" | "black");
    }
    words(func, param).is_some_and(|ws| ws.contains(&word))
}

/// The hint of E0226 for a named argument a web function does not have, where it is a known
/// guess: what to write instead.
pub fn unknown_param_hint(func: &str, param: &str) -> Option<String> {
    Some(match (func, param) {
        ("hero", "rows" | "hours" | "open") => "use `badge: [(\"Mon–Fri\", \"8:00-16:00\")]`".into(),
        (_, "onclick" | "onClick" | "on_click" | "click" | "action") => {
            format!("an event is `on: Msg.X(...)`: `{func}(..., on: Msg.Show(1))`")
        }
        (_, "class" | "className" | "class_name" | "classes") => "use `cls: \"name\"`".into(),
        (_, "style") => "use `cls: \"name\"` and `css(\".name { ... }\")`".to_string(),
        (_, "children" | "content" | "body") => format!("use a child block: `{func}(...) {{ p(\"text\") }}`"),
        _ => return None,
    })
}

/// How the library uses a text argument.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    /// shown as text: inline Markdown, icons and links
    Text,
    /// a link target
    Url,
    /// an image of `assets/` or an https URL
    Image,
    /// an icon name
    Icon,
    /// used as it is (ids, classes, colors, motion words, CSS sizes)
    Plain,
}

/// The kind of parameter `param` of `func`; for a tuple, of its element `elem`.
fn kind(func: &str, param: &str, elem: Option<usize>) -> Kind {
    match (param, elem) {
        ("href" | "to" | "url", _) => Kind::Url,
        ("links" | "buttons" | "back" | "other" | "cta", Some(1)) => Kind::Url,
        ("image" | "src" | "logo" | "poster", _) => Kind::Image,
        ("photos", Some(0)) => Kind::Image,
        ("photos", Some(1)) | ("photos", Some(2)) | ("filters", Some(0)) => Kind::Plain,
        ("rows" | "badge", Some(1)) => Kind::Plain,
        ("icon", _) => Kind::Icon,
        ("hint" | "alt" | "label" | "subject" | "mailto" | "name", _) if func != "quote" => Kind::Plain,
        (
            "id" | "cls" | "anim" | "hover" | "loop" | "scroll" | "morph" | "bg" | "tone" | "align" | "width" | "widths" | "gap"
            | "ratio" | "fit" | "style" | "size" | "type" | "auto" | "value" | "options" | "color" | "path" | "lang" | "tz"
            | "primary" | "accent" | "alt" | "ink" | "muted" | "palette" | "font" | "display" | "motion" | "transition" | "height"
            | "next" | "say" | "raw" | "body",
            _,
        ) => Kind::Plain,
        _ if func == "css" || func == "html" || func == "keyframes" => Kind::Plain,
        _ => Kind::Text,
    }
}

/// Links the library `src` into `prog` (for its `use web` line at `at`); `intrinsics` are the
/// build's hooks the library calls by their short names (`_emit`).
pub fn link(prog: &mut Program, at: Span, src: &str, intrinsics: &[String]) -> Vec<Diag> {
    let lib = library(src);
    let public: HashSet<String> = lib.funcs.iter().filter(|f| !f.name.starts_with('_')).map(|f| f.name.clone()).collect();
    let params: HashMap<String, Vec<String>> =
        lib.funcs.iter().map(|f| (f.name.clone(), f.params.iter().map(|p| p.name.clone()).collect())).collect();
    let icons: HashSet<String> = icons().into_iter().collect();

    let mut errs = clashes(prog, &public);
    if !errs.is_empty() {
        return errs;
    }
    let msg = MsgInfo::of(prog);
    let user: HashSet<String> = prog.funcs.iter().map(|f| f.name.clone()).collect();
    let mut used_on = false;
    let ctx = Ctx { public: &public, params: &params, icons: &icons, msg: &msg };
    for f in prog.funcs.iter_mut() {
        // only the program's own code: not another standard module's functions
        if f.name.contains('.') && !prog.files.contains_key(&f.name) {
            continue;
        }
        each_stmt_mut(&mut f.body, &mut |_| {}, &mut |e| ctx.call(e, &user, &mut errs, &mut used_on));
    }
    for ex in prog.examples.iter_mut() {
        ex.expr.each_mut(&mut |e| ctx.call(e, &user, &mut errs, &mut used_on));
    }
    if lib.funcs.iter().any(|f| f.name == "_live") {
        errs.extend(live(prog, &public));
    }

    // the library, as `web.name`
    let mut own: Vec<String> = lib.funcs.iter().map(|f| f.name.clone()).collect();
    own.extend(intrinsics.iter().cloned());
    let stateful = own.iter().any(|f| f == "_init");
    for mut f in lib.funcs {
        f.name = format!("web.{}", f.name);
        crate::stdlib::rename_calls(&mut f, "web", &own, at);
        prog.funcs.push(f);
    }
    for mut s in lib.structs {
        s.span = at;
        s.fields.iter_mut().for_each(|x| x.span = at);
        prog.structs.push(s);
    }
    if used_on && msg.update {
        let (toks, _) = crate::lexer::lex("fn _on(m: Msg) -> str = json.str(m)\n");
        let (mut p, _) = crate::parser::parse(toks);
        let mut f = p.funcs.remove(0);
        f.name = "web._on".to_string();
        crate::stdlib::rename_calls(&mut f, "web", &[], at);
        prog.funcs.push(f);
    }
    if stateful {
        declare_state(prog, at);
    }
    errs
}

/// True for an error of `link` that leaves the program whole, so the type checker can still run
/// and report its own errors in the same run: a literal value the library does not take (an
/// unknown word E0374, an icon-only button E0377, raw CSS or HTML that is no literal E0378, a
/// forbidden link E0381) or a mistake of the live views (E0376, E0384). A name clash (E0372) or
/// `on:` without `Msg` (E0373) would only cause more errors.
pub fn independent(d: &Diag) -> bool {
    matches!(d.code, "E0374" | "E0377" | "E0381" | "E0376" | "E0384")
        || (d.code == "E0378" && d.msg.contains("takes only a string literal"))
}

/// The motion options and `keyframes(...)` calls of a program's source whose values are literals
/// (`src/web/motion.rs`, `lint`): an unknown motion word is E0374 and a `keyframes` call that
/// cannot make a word E0388, at the place of the value; a keyframes body that animates a layout
/// property is warning E0375 (the build gives it for computed bodies, and skips the ones given here).
pub fn motion(src: &str) -> Vec<Diag> {
    use crate::web::motion::IssueKind;
    let mut out = Vec::new();
    for issue in crate::web::motion::lint_source(src) {
        let (code, fallback) = match issue.kind {
            IssueKind::UnknownWord => ("E0374", "the motion words: `nyra explain E0374`"),
            IssueKind::BadKeyframes => {
                ("E0388", "a keyframes word is `keyframes(\"sway\", \"0%, 100% { rotate: -3deg } 50% { rotate: 3deg }\")`")
            }
            IssueKind::LayoutProperty => {
                let hint = issue.hint.unwrap_or_else(|| "animate `translate`, `scale` or `clip-path` instead".to_string());
                crate::diag::warn(Diag::new("E0375", issue.message, issue.span).hint(hint));
                continue;
            }
        };
        out.push(Diag::new(code, issue.message, issue.span).hint(issue.hint.unwrap_or_else(|| fallback.to_string())));
    }
    out
}

/// E0372: a program function, type or script variable with the name of a web function or type.
fn clashes(prog: &Program, public: &HashSet<String>) -> Vec<Diag> {
    let mut errs = Vec::new();
    for f in &prog.funcs {
        if public.contains(&f.name) && !prog.files.contains_key(&f.name) {
            let new = new_name(&f.name);
            errs.push(
                Diag::new("E0372", format!("`fn {}` has the name of the web function `{}`", f.name, f.name), f.span)
                    .hint(format!("rename yours, e.g. `fn {new}`"))
                    .fix(rename(prog, f, &new, public).unwrap_or_default()),
            );
        }
    }
    let types = prog.structs.iter().map(|s| (&s.name, s.span)).chain(prog.enums.iter().map(|e| (&e.name, e.span)));
    for (name, span) in types {
        if TYPES.contains(&name.as_str()) {
            errs.push(
                Diag::new("E0372", format!("the type `{name}` has the name of the web type `{name}`"), span)
                    .hint(format!("rename your type, e.g. `My{name}`: `use web` defines `{name}`")),
            );
        }
    }
    if let Some(main) = prog.funcs.iter().find(|f| f.name == "main").filter(|_| prog.script) {
        for s in &main.body {
            if let StmtKind::Let { name, mutable, .. } = &s.kind {
                if public.contains(name) || name == STATE {
                    let what = if name == STATE { "the site's state".to_string() } else { format!("the web function `{name}`") };
                    errs.push(
                        Diag::new("E0372", format!("the script variable `{name}` has the name of {what}"), s.span)
                            .hint(format!("rename it, e.g. `{} my_{name} = ...`", if *mutable { "var" } else { "let" })),
                    );
                }
            }
        }
    }
    errs
}

/// The name E0372 suggests for a program function named like a web function.
fn new_name(name: &str) -> String {
    match name {
        "img" | "image" => "photo".to_string(),
        _ => format!("my_{name}"),
    }
}

/// The edits that rename the program's `fn old` (named like a web function) to `new`, and every call
/// of it, or `None` when that is not certain: a call whose argument count does not fit the
/// function (it may mean the web function), the old name used as a variable, `new` already a name
/// of the program, or a program of several files.
fn rename(prog: &Program, f: &Func, new: &str, public: &HashSet<String>) -> Option<Vec<Edit>> {
    if !prog.files.is_empty() || public.contains(new) {
        return None;
    }
    let old = f.name.as_str();
    let required = f.params.iter().filter(|p| p.default.is_none()).count();
    let mut edits = vec![Edit::replace(f.span, old, new)];
    // every name the program uses or declares, apart from the calls of `old`
    let mut names: HashSet<String> = HashSet::new();
    let mut certain = true;
    let mut visit = |e: &mut Expr| match &e.kind {
        ExprKind::Call(n, args) if n == old => {
            let fits = args.len() >= required && args.len() <= f.params.len();
            certain &= fits && !args.iter().any(|a| matches!(a.kind, ExprKind::Inout(_)));
            edits.push(Edit::replace(e.span, old, new));
        }
        ExprKind::Call(n, _) | ExprKind::Var(n) | ExprKind::Bind(n, ..) => {
            names.insert(n.clone());
        }
        ExprKind::Lambda(ps, _) => names.extend(ps.iter().map(|(p, _)| p.clone())),
        _ => {}
    };
    let own = || prog.funcs.iter().filter(|g| !g.name.contains('.'));
    let fns: Vec<&str> = own().map(|g| g.name.as_str()).collect();
    let mut bound: Vec<String> = own().flat_map(|g| g.params.iter().map(|p| p.name.clone())).collect();
    let mut stmt = |s: &mut Stmt| match &s.kind {
        StmtKind::Let { name, .. } | StmtKind::For { var: name, .. } => bound.push(name.clone()),
        StmtKind::ForEach { var, index, .. } => bound.extend(std::iter::once(var.clone()).chain(index.clone())),
        _ => {}
    };
    for g in own() {
        let mut body = g.body.clone();
        each_stmt_mut(&mut body, &mut stmt, &mut visit);
    }
    for ex in &prog.examples {
        ex.expr.clone().each_mut(&mut visit);
    }
    names.extend(bound);
    names.extend(prog.structs.iter().map(|s| s.name.clone()).chain(prog.enums.iter().map(|e| e.name.clone())));
    // `old` as a variable or a parameter is not the function (a local may have a web function's name)
    if !certain || names.contains(old) || names.contains(new) || fns.contains(&new) || fns.iter().filter(|g| **g == old).count() > 1 {
        return None;
    }
    Some(edits)
}

/// What the program declares for events: `enum Msg` and its variants, and `fn update(m: Msg)`.
struct MsgInfo {
    /// the variants of `enum Msg` and whether each carries values (`None`: no `enum Msg`)
    variants: Option<Vec<(String, bool)>>,
    update: bool,
}

impl MsgInfo {
    fn of(prog: &Program) -> MsgInfo {
        let variants = prog
            .enums
            .iter()
            .find(|e| e.name == "Msg")
            .map(|e| e.variants.iter().map(|v| (v.name.clone(), !v.fields.is_empty())).collect());
        let update = variants.is_some()
            && prog
                .funcs
                .iter()
                .any(|f| f.name == "update" && f.params.len() == 1 && f.params[0].ty.name() == "Msg" && !f.params[0].inout);
        MsgInfo { variants, update }
    }
}

struct Ctx<'a> {
    public: &'a HashSet<String>,
    params: &'a HashMap<String, Vec<String>>,
    icons: &'a HashSet<String>,
    msg: &'a MsgInfo,
}

impl Ctx<'_> {
    /// A call of a web function by its bare name: renamed, its `on:` made JSON, its literals checked.
    fn call(&self, e: &mut Expr, user: &HashSet<String>, errs: &mut Vec<Diag>, used_on: &mut bool) {
        let span = e.span;
        let ExprKind::Call(name, args) = &mut e.kind else { return };
        if name == "Node" || name == "WebConfig" {
            errs.push(
                Diag::new("E0378", format!("a `{name}` is made by the web functions, not written out"), span)
                    .hint("use the elements (`p(...)`, `card(...)`) or `html(\"...\")`"),
            );
            return;
        }
        if !self.public.contains(name.as_str()) || user.contains(name.as_str()) {
            return;
        }
        let func = name.clone();
        *name = format!("web.{func}");
        let names = &self.params[&func];
        let given: Vec<String> =
            args.iter().map(|a| if let ExprKind::Labeled(l, _) = &a.kind { l.clone() } else { String::new() }).collect();
        for (i, a) in args.iter_mut().enumerate() {
            let (param, value) = match &mut a.kind {
                ExprKind::Labeled(l, v) => (l.clone(), &mut **v),
                ExprKind::Block(..) => continue,
                _ => (names.get(i).cloned().unwrap_or_default(), a),
            };
            if param == "on" {
                *used_on = true;
                self.on(&func, value, errs);
                continue;
            }
            if func == "gallery" && param == "photos" {
                tag_pairs(value);
            }
            self.literal(&func, &param, value, None, errs);
        }
        // an icon-only button or link needs a label for screen readers
        if matches!(func.as_str(), "button" | "link") && !given.iter().any(|g| g == "label") {
            let text = args.iter().enumerate().find_map(|(i, a)| match &a.kind {
                ExprKind::Labeled(l, v) if l == "text" => Some(&**v),
                ExprKind::Labeled(..) | ExprKind::Block(..) => None,
                _ if i == 0 => Some(a),
                _ => None,
            });
            let icon = args.iter().any(|a| matches!(&a.kind, ExprKind::Labeled(l, _) if l == "icon"));
            let empty = match text.map(|t| &t.kind) {
                Some(ExprKind::Str(s)) => strip_icons(s).trim().is_empty() && (icon || s.contains(':')),
                None => icon,
                _ => false,
            };
            if empty {
                errs.push(
                    Diag::new("E0377", format!("this `{func}` shows only an icon, so a screen reader has nothing to read"), span)
                        .hint(format!("say what it does with `label:`, e.g. `{func}(..., label: \"Call us\")`")),
                );
            }
        }
    }

    /// `on: Msg.X(..)`: the JSON of the value, made by `web._on`; a form's `on: Msg.Send` (a variant
    /// that carries the fields) is written without its value, as the JSON string `"Send"`.
    fn on(&self, func: &str, value: &mut Expr, errs: &mut Vec<Diag>) {
        if !self.msg.update {
            let missing = if self.msg.variants.is_none() { "no `enum Msg`" } else { "no `fn update(m: Msg)`" };
            errs.push(
                Diag::new("E0373", format!("`on:` sends a `Msg` to `update`, but the program has {missing}"), value.span)
                    .hint("declare `enum Msg { Show(str) }` and `fn update(m: Msg) { match m { Msg.Show(k) => kind = k } }`"),
            );
            return;
        }
        if matches!(func, "form" | "field") {
            if let ExprKind::Field(base, variant) = &value.kind {
                let carries = self.msg.variants.as_ref().and_then(|vs| vs.iter().find(|(n, _)| n == variant)).is_some_and(|(_, c)| *c);
                if matches!(&base.kind, ExprKind::Var(m) if m == "Msg") && carries {
                    value.kind = ExprKind::Str(format!("\"{variant}\""));
                    return;
                }
            }
        }
        let inner = std::mem::replace(value, Expr::new(ExprKind::None, value.span));
        *value = Expr::new(ExprKind::Call("web._on".to_string(), vec![inner]), value.span);
    }

    /// Checks a literal argument (or the literal parts of one) of parameter `param` of `func`.
    fn literal(&self, func: &str, param: &str, value: &Expr, elem: Option<usize>, errs: &mut Vec<Diag>) {
        let k = kind(func, param, elem);
        match &value.kind {
            ExprKind::Str(s) => self.text(func, param, k, s, value.span, true, errs),
            ExprKind::Interp(parts) => {
                if matches!(func, "html" | "css") {
                    raw_needs_literal(func, value.span, errs);
                    return;
                }
                for (i, p) in parts.iter().enumerate() {
                    if let InterpPart::Lit(s) = p {
                        self.text(func, param, k, s, value.span, i == 0, errs);
                    }
                }
            }
            ExprKind::Array(xs) => xs.iter().for_each(|x| self.literal(func, param, x, None, errs)),
            ExprKind::Tuple(xs) => xs.iter().enumerate().for_each(|(i, x)| self.literal(func, param, x, Some(i), errs)),
            _ if matches!(func, "html" | "css") && matches!(param, "raw" | "text") => raw_needs_literal(func, value.span, errs),
            _ => {}
        }
    }

    /// One piece of literal text. `start`: it is the start of the value (where a scheme would be).
    #[allow(clippy::too_many_arguments)]
    fn text(&self, func: &str, param: &str, k: Kind, s: &str, span: Span, start: bool, errs: &mut Vec<Diag>) {
        match k {
            Kind::Url | Kind::Image if start => {
                if let Some(bad) = forbidden_scheme(s, k == Kind::Image) {
                    errs.push(scheme_error(&bad, s, span));
                }
            }
            Kind::Icon if start && !s.is_empty() && !self.icons.contains(s) => {
                // `icon: ":clock:"`: the colons that mark an icon in text, around a known name
                let bare = s.strip_prefix(':').and_then(|x| x.strip_suffix(':')).filter(|x| self.icons.contains(*x));
                match bare {
                    Some(name) => errs.push(
                        Diag::new("E0374", format!("unknown icon `{s}`"), span)
                            .hint(format!("no colons in `{param}:`"))
                            .fix(vec![Edit::replace(span, &format!("\"{s}\""), format!("\"{name}\""))]),
                    ),
                    // an icon is decoration: the site still builds (and draws a stand-in)
                    None => crate::diag::warn(icon_warning(s, false, span)),
                }
            }
            Kind::Text => {
                for name in icon_names(s) {
                    if !self.icons.contains(&name) {
                        crate::diag::warn(icon_warning(&name, true, span));
                    }
                }
                for url in md_links(s) {
                    if let Some(bad) = forbidden_scheme(&url, false) {
                        errs.push(scheme_error(&bad, &url, span));
                    }
                }
            }
            Kind::Plain if start => {
                if let Some(ws) = words(func, param) {
                    if !s.is_empty() && !ws.contains(&s) {
                        let hint = match suggest(s, ws.iter().copied()) {
                            Some(h) => format!("{h} The words of `{param}:` are {}", list(ws)),
                            None => format!("the words of `{param}:` are {}", list(ws)),
                        };
                        errs.push(Diag::new("E0374", format!("unknown word `{s}` for `{param}:` of `{func}`"), span).hint(hint));
                    }
                }
            }
            _ => {}
        }
    }
}

/// `gallery([(image, alt), ...])`: a literal list of pairs, each completed with the empty tag (a
/// list of pairs in a variable is mapped by the checker, `photo_pairs`).
fn tag_pairs(value: &mut Expr) {
    let ExprKind::Array(items) = &mut value.kind else { return };
    if items.is_empty() || !items.iter().all(|x| matches!(&x.kind, ExprKind::Tuple(parts) if parts.len() == 2)) {
        return;
    }
    for x in items.iter_mut() {
        let at = x.span;
        if let ExprKind::Tuple(parts) = &mut x.kind {
            parts.push(Expr::new(ExprKind::Str(String::new()), at));
        }
    }
}

fn raw_needs_literal(func: &str, span: Span, errs: &mut Vec<Diag>) {
    errs.push(Diag::new("E0378", format!("`{func}(...)` takes only a string literal"), span).hint(format!(
        "write the {} as one literal without `{{ }}`; for data use `p(text)`",
        if func == "css" { "CSS" } else { "markup" }
    )));
}

/// The edit distance of two names (a swap of neighbours counts as one edit).
fn edits(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
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

fn scheme_error(scheme: &str, url: &str, span: Span) -> Diag {
    Diag::new("E0381", format!("the link `{url}` uses the forbidden scheme `{scheme}:`"), span)
        .hint("link to `https://`, `mailto:`, `tel:`, a path (`/about`) or `#anchor`")
}

fn list(ws: &[&str]) -> String {
    ws.iter().map(|w| format!("`{w}`")).collect::<Vec<_>>().join(", ")
}

/// The scheme of `url` if a page may not use it (images: only http and https).
fn forbidden_scheme(url: &str, image: bool) -> Option<String> {
    let u = url.trim();
    let mut scheme = String::new();
    for (i, c) in u.chars().enumerate() {
        if c == ':' {
            break;
        }
        if !(c.is_ascii_alphabetic() || (i > 0 && (c.is_ascii_digit() || matches!(c, '+' | '-' | '.')))) {
            return None;
        }
        scheme.push(c.to_ascii_lowercase());
    }
    if scheme.is_empty() || scheme.len() == u.len() {
        return None;
    }
    let ok: &[&str] = if image { &["http", "https"] } else { &["http", "https", "mailto", "tel"] };
    (!ok.contains(&scheme.as_str())).then_some(scheme)
}

/// The icon names `:name:` in a text (the same rule as the library's `_md`).
fn icon_names(s: &str) -> Vec<String> {
    let cs: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        if cs[i] == '\\' {
            i += 2;
            continue;
        }
        if cs[i] == ':' && i + 1 < cs.len() && cs[i + 1].is_ascii_lowercase() {
            let mut j = i + 1;
            while j < cs.len() && (cs[j].is_ascii_lowercase() || cs[j].is_ascii_digit() || cs[j] == '-') {
                j += 1;
            }
            if j < cs.len() && cs[j] == ':' {
                out.push(cs[i + 1..j].iter().collect());
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// A text without its icons.
fn strip_icons(s: &str) -> String {
    let mut out = s.to_string();
    for name in icon_names(s) {
        out = out.replace(&format!(":{name}:"), "");
    }
    out
}

/// The targets of the Markdown links `[label](url)` in a text.
fn md_links(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(i) = rest.find("](") {
        let after = &rest[i + 2..];
        match after.find(')') {
            Some(j) if !after[..j].contains('\n') => {
                out.push(after[..j].to_string());
                rest = &after[j..];
            }
            _ => rest = after,
        }
    }
    out
}

/// Declares the site's state, `var _web = web._init()`, before the program's first statement. A
/// program that has only `fn main` gets a script `main` that declares it and calls the program's.
fn declare_state(prog: &mut Program, at: Span) {
    let decl = Stmt {
        kind: StmtKind::Let {
            name: STATE.to_string(),
            mutable: true,
            ty: None,
            value: Expr::new(ExprKind::Call("web._init".to_string(), Vec::new()), at),
        },
        span: at,
    };
    if prog.script {
        if let Some(main) = prog.funcs.iter_mut().find(|f| f.name == "main") {
            main.body.insert(0, decl);
        }
        return;
    }
    let Some(user) = prog.funcs.iter_mut().find(|f| f.name == "main") else { return };
    user.name = USER_MAIN.to_string();
    let call = Stmt { kind: StmtKind::Expr(Expr::new(ExprKind::Call(USER_MAIN.to_string(), Vec::new()), at)), span: at };
    prog.funcs.push(Func {
        name: "main".to_string(),
        params: Vec::new(),
        ret: Type::Void,
        body: vec![decl, call],
        span: at,
        contract: Default::default(),
    });
    prog.script = true;
}

// ---------------------------------------------------------------------------------------------
// Live regions

/// What one function does with the script variables, directly.
#[derive(Default)]
struct Facts {
    reads: BTreeSet<String>,
    writes: BTreeSet<String>,
    calls: BTreeSet<String>,
    /// `fs`, `os` or `input` calls: (module, span)
    effects: Vec<(String, Span)>,
}

/// Finds the live views and wraps their calls (see the module comment); E0376 and E0384.
fn live(prog: &mut Program, public: &HashSet<String>) -> Vec<Diag> {
    let mut errs = Vec::new();
    if !prog.funcs.iter().any(|f| f.name == "update") || !prog.script {
        return errs;
    }
    let globals: HashSet<String> = match prog.funcs.iter().find(|f| f.name == "main") {
        Some(m) => {
            m.body.iter().filter_map(|s| if let StmtKind::Let { name, .. } = &s.kind { Some(name.clone()) } else { None }).collect()
        }
        None => return errs,
    };
    let names: HashSet<String> = prog.funcs.iter().map(|f| f.name.clone()).collect();
    let mut facts: HashMap<String, Facts> = HashMap::new();
    for f in &prog.funcs {
        if f.name == "main" || (f.name.contains('.') && !prog.files.contains_key(&f.name)) {
            continue;
        }
        facts.insert(f.name.clone(), facts_of(f, &globals, &names, public));
    }
    let closure = |start: &str, pick: &dyn Fn(&Facts) -> &BTreeSet<String>| -> BTreeSet<String> {
        let mut seen: HashSet<String> = HashSet::new();
        let mut out = BTreeSet::new();
        let mut todo = vec![start.to_string()];
        while let Some(n) = todo.pop() {
            if !seen.insert(n.clone()) {
                continue;
            }
            if let Some(fa) = facts.get(&n) {
                out.extend(pick(fa).iter().cloned());
                todo.extend(fa.calls.iter().cloned());
            }
        }
        out
    };
    let changed = closure("update", &|f| &f.writes);
    if changed.is_empty() {
        return errs;
    }
    stale_reads(prog, &changed);
    let mut views: Vec<String> = Vec::new();
    let mut with_params: Vec<String> = Vec::new();
    for f in &prog.funcs {
        if !facts.contains_key(&f.name) || f.name == "update" || f.ret.name() != "Node" {
            continue;
        }
        if closure(&f.name, &|x| &x.reads).iter().any(|v| changed.contains(v)) {
            if f.params.is_empty() {
                views.push(f.name.clone());
            } else {
                with_params.push(f.name.clone());
            }
        }
    }
    // the code that runs in the browser: `update`, the views and what they call
    let mut client: Vec<String> = vec!["update".to_string()];
    client.extend(views.iter().cloned());
    let mut seen: HashSet<String> = HashSet::new();
    while let Some(n) = client.pop() {
        if !seen.insert(n.clone()) {
            continue;
        }
        if let Some(fa) = facts.get(&n) {
            for (m, span) in &fa.effects {
                errs.push(
                    Diag::new("E0384", format!("`{n}` runs in the browser but uses `{m}`"), *span)
                        .hint("read it at build time into a script variable"),
                );
            }
            client.extend(fa.calls.iter().cloned());
        }
    }
    for f in prog.funcs.iter_mut() {
        if views.contains(&f.name) || f.name == "update" || (f.name.contains('.') && !prog.files.contains_key(&f.name)) {
            continue;
        }
        each_stmt_mut(&mut f.body, &mut |_| {}, &mut |e| {
            let ExprKind::Call(name, args) = &e.kind else { return };
            if args.is_empty() && views.contains(name) {
                let name = name.clone();
                let span = e.span;
                let inner = std::mem::replace(e, Expr::new(ExprKind::None, span));
                // (the inner call is renamed back below: this walk goes on into the new call)
                let inner = match inner.kind {
                    ExprKind::Call(n, a) => Expr::new(ExprKind::Call(format!("{HELD}{n}"), a), span),
                    k => Expr::new(k, span),
                };
                *e = Expr::new(ExprKind::Call("web._live".to_string(), vec![Expr::new(ExprKind::Str(name), span), inner]), span);
            } else if with_params.contains(name) {
                errs.push(
                    Diag::new(
                        "E0376",
                        format!("live view `{name}` (it reads state `update` changes) must have no parameters"),
                        e.span,
                    )
                    .hint(format!("let `fn {name}()` read the script variables itself")),
                );
            }
        });
    }
    for f in prog.funcs.iter_mut() {
        each_stmt_mut(&mut f.body, &mut |_| {}, &mut |e| {
            if let ExprKind::Call(n, _) = &mut e.kind {
                if let Some(plain) = n.strip_prefix(HELD) {
                    *n = plain.to_string();
                }
            }
        });
    }
    errs
}

/// Warning E0391: a page that reads a script variable `update` changes right in its sections
/// (`p("Showing {kind}")` in `page(...) { ... }`): the page is rendered once, while the site is
/// built, so that part never changes. Only a live view (`fn view() -> Node`) renders again.
fn stale_reads(prog: &Program, changed: &BTreeSet<String>) {
    let Some(main) = prog.funcs.iter().find(|f| f.name == "main") else { return };
    let mut seen: Vec<String> = Vec::new();
    let mut body = main.body.clone();
    each_stmt_mut(&mut body, &mut |_| {}, &mut |e| {
        let ExprKind::Call(n, _) = &e.kind else { return };
        if n != "web.page" {
            return;
        }
        e.clone().each_mut(&mut |x| {
            if let ExprKind::Var(v) = &x.kind {
                if changed.contains(v) && !seen.contains(v) {
                    seen.push(v.clone());
                    crate::diag::warn(
                        Diag::new("E0391", format!("the page reads `{v}` once, at build time: `update` changes it in vain"), x.span)
                            .hint(format!("read it in a live view, `fn view() -> Node` that uses `{v}`, and call `view()` here")),
                    );
                }
            }
        });
    });
}

/// Marks a call of a live view while its calls are being wrapped.
const HELD: &str = "\u{1}";

fn facts_of(f: &Func, globals: &HashSet<String>, funcs: &HashSet<String>, public: &HashSet<String>) -> Facts {
    let mut own: HashSet<String> = f.params.iter().map(|p| p.name.clone()).collect();
    let mut decls = Vec::new();
    crate::check::collect_decls(&f.body, &mut decls);
    own.extend(decls.into_iter().map(|(n, _)| n));
    let mut fa = Facts::default();
    let global = |n: &str| globals.contains(n) && !own.contains(n);
    let mut body = f.body.clone();
    let mut assigned = Vec::new();
    each_stmt_mut(
        &mut body,
        &mut |s| {
            if let StmtKind::Assign { target, .. } = &s.kind {
                if let Some(r) = root(target).filter(|r| global(r)) {
                    assigned.push(r);
                }
            }
        },
        &mut |e| match &e.kind {
            ExprKind::Var(n) if global(n) => {
                fa.reads.insert(n.clone());
            }
            ExprKind::Call(n, args) => {
                if funcs.contains(n) && !public.contains(n) {
                    fa.calls.insert(n.clone());
                }
                for a in args {
                    if let ExprKind::Inout(p) = &a.kind {
                        if let Some(r) = root(p).filter(|r| global(r)) {
                            fa.writes.insert(r);
                        }
                    }
                }
            }
            ExprKind::Method(recv, m, _) => {
                if let ExprKind::Var(module) = &recv.kind {
                    if matches!(module.as_str(), "fs" | "os" | "input") && !own.contains(module) && !globals.contains(module) {
                        fa.effects.push((format!("{module}.{m}"), e.span));
                    }
                }
                if MUTATORS.contains(&m.as_str()) {
                    if let Some(r) = root(recv).filter(|r| global(r)) {
                        fa.writes.insert(r);
                    }
                }
            }
            _ => {}
        },
    );
    fa.writes.extend(assigned);
    fa
}

/// The methods that change the array or map they are called on.
const MUTATORS: &[&str] = &["push", "pop", "insert", "remove", "swap", "sort", "reverse", "set", "sort_by", "clear"];

/// The variable at the root of a place: `a` of `a.b[3].c`.
fn root(e: &Expr) -> Option<String> {
    match &e.kind {
        ExprKind::Var(n) => Some(n.clone()),
        ExprKind::Field(b, _) | ExprKind::Index(b, _) => root(b),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_library_parses_and_has_its_icons() {
        let fs = functions();
        for f in ["site", "page", "section", "hero", "navbar", "gallery", "hours", "map", "form", "field", "footer"] {
            assert!(fs.iter().any(|(n, _)| n == f), "{f}");
        }
        let icons = icons();
        for i in
            ["phone", "mail", "map-pin", "clock", "leaf", "flame", "home", "facebook", "send", "navigation", "alert-circle", "circle"]
        {
            assert!(icons.iter().any(|n| n == i), "{i}");
        }
        // a name of another set is no icon of the set, but stands for one
        assert!(!icons.iter().any(|n| n == "envelope"));
        assert_eq!(icon_like("envelope"), Some("mail"));
        assert_eq!(icon_like("map-marker"), Some("map-pin"));
        assert_eq!(icon_like("siren"), None);
        for (other, icon) in [("warning", "triangle-alert"), ("cog", "settings")] {
            assert_eq!(icon_like(other), Some(icon));
            assert!(icons.iter().any(|n| n == icon), "{icon}");
        }
    }

    #[test]
    fn schemes_and_markdown_links() {
        assert_eq!(forbidden_scheme("javascript:alert(1)", false).as_deref(), Some("javascript"));
        assert_eq!(forbidden_scheme(" JavaScript:x", false).as_deref(), Some("javascript"));
        assert_eq!(forbidden_scheme("data:text/html,x", false).as_deref(), Some("data"));
        assert_eq!(forbidden_scheme("mailto:a@b.cz", false), None);
        assert_eq!(forbidden_scheme("mailto:a@b.cz", true).as_deref(), Some("mailto"));
        assert_eq!(forbidden_scheme("/a:b", false), None);
        assert_eq!(forbidden_scheme("#kontakt", false), None);
        assert_eq!(md_links("[a](https://x.cz) and [b](javascript:x)"), vec!["https://x.cz", "javascript:x"]);
        assert_eq!(icon_names(":phone: 8:00 \\:x: :map-pin:"), vec!["phone", "map-pin"]);
    }
}
