//! Understanding a page for `nyra clone`: the DOM and its styles become a `Page`, a model in the
//! terms of the web kit (navbar, hero, sections of cards, galleries, quotes, stats, steps, forms,
//! opening hours, maps, a footer) with every visible text, image and link of the original.
//!
//! The text of the model is already the kit's inline Markdown (`**bold**`, `[label](url)`), with
//! the characters Markdown would read escaped; `write.rs` only has to make Nyra string literals.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use super::css::{self, Rgb, Styles};
use super::dom::Dom;
pub use super::motion::Mo;
use super::motion::Motions;

// ---- the model ------------------------------------------------------------------------------------

#[derive(Default)]
pub struct Page {
    pub name: String,
    pub title: String,
    pub desc: String,
    pub lang: String,
    pub theme: Theme,
    pub nav: Option<Nav>,
    pub sections: Vec<Section>,
    pub footer: Option<Foot>,
    /// What was left out, by kind, with counts.
    pub dropped: BTreeMap<String, usize>,
    /// Local images to copy into `assets/`: (file, name).
    pub assets: Vec<(PathBuf, String)>,
    /// The words of visible text on the original page, in order.
    pub words: Vec<String>,
    /// Image URLs on the original page (content images) and in the model.
    pub images: Vec<String>,
    /// An app's side navigation (a column of links beside the content).
    pub side: Option<Side>,
    /// Custom motion words: (name, keyframes body).
    pub keyframes: Vec<(String, String)>,
    /// The motion found on the page and the word it became, with counts.
    pub motion: BTreeMap<(String, String), usize>,
    /// What the page does that needs a server ("sign-in and accounts", "comments", ...).
    pub server: Vec<String>,
    /// `// TODO(server)` notes for the top of the page (features of the header).
    pub todo_top: Vec<String>,
    /// The videos that play in the clone (players, embeds, video thumbnails).
    pub videos: usize,
}

#[derive(Default, Clone)]
pub struct Side {
    /// (label, href); an empty href is a group heading or a note.
    pub links: Vec<(String, String)>,
}

/// A thumbnail of a grid of videos or products.
#[derive(Default, Clone)]
pub struct MediaItem {
    pub title: String,
    pub image: String,
    pub alt: String,
    pub href: String,
    pub meta: String,
    pub badge: String,
    pub price: String,
    pub tag: String,
}

#[derive(Default, Clone)]
pub struct Theme {
    pub primary: Option<String>,
    pub accent: Option<String>,
    pub bg: Option<String>,
    pub ink: Option<String>,
    pub font: Option<String>,
    pub display: Option<String>,
    pub radius: Option<u32>,
    pub dark: bool,
    /// The text runs in a narrow column (a `max-width` of 60rem or less around it).
    pub narrow: bool,
    /// Section headings are left-aligned (the kit centers them).
    pub left: bool,
}

#[derive(Default)]
pub struct Nav {
    pub brand: String,
    pub logo: String,
    pub links: Vec<(String, String)>,
    pub cta: Option<(String, String)>,
}

#[derive(Default)]
pub struct Foot {
    pub id: String,
    pub brand: String,
    pub text: String,
    pub links: Vec<(String, String)>,
    pub links_title: String,
    pub contact: Vec<String>,
    pub small: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Hero,
    Split,
    Plain,
}

pub struct Section {
    pub kind: Kind,
    pub id: String,
    pub title: String,
    pub kicker: String,
    pub text: String,
    pub bg: String,
    pub image: String,
    pub alt: String,
    pub buttons: Vec<(String, String)>,
    pub items: Vec<Item>,
    pub panels: Vec<Card>,
    /// The heading level of the title in the original (1 for the page's h1).
    pub level: u8,
    /// How the section (or hero) moves.
    pub mo: Mo,
    /// A hero's picture beside its text (a dish, a product), not behind it: (src, alt).
    pub aside: Option<(String, String, Mo)>,
}

#[derive(Clone)]
pub enum Item {
    P(String),
    /// A heading: level, text, and its id in the clone when links point to it.
    H(u8, String, String),
    Img {
        src: String,
        alt: String,
        caption: String,
        mo: Mo,
        /// A width in px for a small picture (an avatar, a logo), "" for the full width.
        width: String,
        /// The width the browser drew it at (0: not known).
        px: u32,
    },
    Buttons(Vec<(String, String)>),
    List {
        items: Vec<String>,
        ordered: bool,
    },
    Table {
        head: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    Cards {
        cols: usize,
        cards: Vec<Card>,
    },
    Gallery(Vec<(String, String)>),
    Quotes(Vec<Quote>),
    Stats(Vec<(String, String)>),
    Steps(Vec<(String, String)>),
    Form(Form),
    Hours(Vec<(String, String)>),
    Map(String),
    Embed {
        url: String,
        title: String,
    },
    Address(Vec<String>),
    Code(String),
    /// An item with motion options (a grid that staggers, a gallery that rises).
    Moved {
        mo: Mo,
        item: Box<Item>,
    },
    /// A grid of video or product thumbnails; `cats`: the categories to filter by.
    Media {
        cols: usize,
        items: Vec<MediaItem>,
        cats: Vec<String>,
    },
    /// A video that plays: a file, or a YouTube or Vimeo address.
    Video {
        src: String,
        poster: String,
        title: String,
    },
    Tabs {
        labels: Vec<String>,
        panels: Vec<Vec<Item>>,
    },
    /// An endless strip of words.
    Marquee(Vec<String>),
    /// An endless strip of pictures (logos): (src, alt).
    Strip(Vec<(String, String)>),
    /// A search box that filters the next item.
    Search(String),
    /// Tabs whose other panels are other pages: (label, href, the current one).
    TabLinks(Vec<(String, String, bool)>),
    /// A note for the program: a feature here needs a server.
    Todo(String),
}

#[derive(Default, Clone)]
pub struct Card {
    pub title: String,
    pub text: String,
    pub image: String,
    pub alt: String,
    pub href: String,
    pub tag: String,
    /// A hex color of the card's own (a split screen's panel).
    pub color: String,
    pub items: Vec<String>,
    pub buttons: Vec<(String, String)>,
    /// Content that is not a title, text, image, list or button (a table, a nested grid, ...).
    pub extra: Vec<Item>,
}

#[derive(Default, Clone)]
pub struct Quote {
    pub text: String,
    pub name: String,
    pub place: String,
}

#[derive(Default, Clone)]
pub struct Form {
    pub title: String,
    pub mailto: String,
    pub to: String,
    pub send: String,
    pub fields: Vec<Field>,
}

#[derive(Default, Clone)]
pub struct Field {
    pub name: String,
    pub label: String,
    pub kind: String,
    pub req: bool,
    pub hint: String,
    pub options: Vec<String>,
}

impl Item {
    /// The kit component this item becomes (for the summary).
    pub fn component(&self) -> &'static str {
        match self {
            Item::P(_) => "p",
            Item::H(..) => "h3",
            Item::Img { .. } => "img",
            Item::Buttons(_) => "button",
            Item::List { .. } => "list",
            Item::Table { .. } => "table",
            Item::Cards { .. } => "card",
            Item::Gallery(_) => "gallery",
            Item::Quotes(_) => "quote",
            Item::Stats(_) => "stat",
            Item::Steps(_) => "steps",
            Item::Form(_) => "form",
            Item::Hours(_) => "hours",
            Item::Map(_) => "map",
            Item::Embed { .. } => "embed",
            Item::Address(_) => "address",
            Item::Code(_) => "html",
            Item::Moved { item, .. } => item.component(),
            Item::Media { .. } => "media",
            Item::Video { .. } => "video",
            Item::Tabs { .. } => "tabs",
            Item::Marquee(_) => "marquee",
            Item::Strip(_) => "marquee",
            Item::Search(_) => "search",
            Item::TabLinks(_) => "button",
            Item::Todo(_) => "TODO(server)",
        }
    }
}

// ---- text helpers --------------------------------------------------------------------------------

/// Text made safe for the kit's inline Markdown: `\`, `*`, `` ` `` and `[` are escaped, and a
/// `:` that would start an icon name (`:word:`).
pub fn md_escape(s: &str) -> String {
    let cs: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    for (i, &c) in cs.iter().enumerate() {
        match c {
            '\\' | '*' | '`' | '[' => {
                out.push('\\');
                out.push(c);
            }
            ':' if cs.get(i + 1).is_some_and(|n| n.is_ascii_lowercase()) => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// Whitespace runs (and no-break spaces) as one space, trimmed.
pub fn collapse(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if c.is_whitespace() || c == '\u{a0}' {
            space = true;
        } else {
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            out.push(c);
        }
    }
    out
}

/// Markdown back to plain text (for alt texts, labels of tuples that are not Markdown).
pub fn md_plain(s: &str) -> String {
    let cs: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c == '\\' && i + 1 < cs.len() {
            out.push(cs[i + 1]);
            i += 2;
            continue;
        }
        if c == '*' || c == '`' {
            i += 1;
            continue;
        }
        if c == '[' {
            // [label](url) -> label
            if let Some(mid) = (i + 1..cs.len().saturating_sub(1)).find(|&k| cs[k] == ']' && cs[k + 1] == '(') {
                if let Some(end) = (mid + 2..cs.len()).find(|&k| cs[k] == ')') {
                    out.push_str(&md_plain(&cs[i + 1..mid].iter().collect::<String>()));
                    i = end + 1;
                    continue;
                }
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

pub fn words(s: &str) -> usize {
    s.split_whitespace().filter(|w| w.chars().any(|c| c.is_alphanumeric())).count()
}

/// A URL inside `[label](url)`: no spaces or parentheses that would end it.
fn md_url(u: &str) -> String {
    let lower = u.to_ascii_lowercase();
    if lower.starts_with("tel:") {
        return u.chars().filter(|c| !c.is_whitespace() && *c != '(' && *c != ')').collect();
    }
    u.replace(' ', "%20").replace('(', "%28").replace(')', "%29").replace(['\n', '\r', '\t'], "")
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// A slug for an id: lowercase ASCII letters, digits and dashes.
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        let c = fold_accent(c).to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').chars().take(40).collect::<String>().trim_end_matches('-').to_string()
}

pub fn fold_accent(c: char) -> char {
    match c {
        'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'ą' => 'a',
        'Á' | 'À' | 'Â' | 'Ä' | 'Ã' | 'Å' | 'Ą' => 'A',
        'č' | 'ç' | 'ć' => 'c',
        'Č' | 'Ç' | 'Ć' => 'C',
        'ď' => 'd',
        'Ď' => 'D',
        'é' | 'è' | 'ê' | 'ë' | 'ě' | 'ę' => 'e',
        'É' | 'È' | 'Ê' | 'Ë' | 'Ě' | 'Ę' => 'E',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'Í' | 'Ì' | 'Î' | 'Ï' => 'I',
        'ľ' | 'ĺ' | 'ł' => 'l',
        'Ľ' | 'Ĺ' | 'Ł' => 'L',
        'ň' | 'ñ' | 'ń' => 'n',
        'Ň' | 'Ñ' | 'Ń' => 'N',
        'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ø' => 'o',
        'Ó' | 'Ò' | 'Ô' | 'Ö' | 'Õ' | 'Ø' => 'O',
        'ř' | 'ŕ' => 'r',
        'Ř' | 'Ŕ' => 'R',
        'š' | 'ś' | 'ß' => 's',
        'Š' | 'Ś' => 'S',
        'ť' => 't',
        'Ť' => 'T',
        'ú' | 'ù' | 'û' | 'ü' | 'ů' => 'u',
        'Ú' | 'Ù' | 'Û' | 'Ü' | 'Ů' => 'U',
        'ý' | 'ÿ' => 'y',
        'Ý' => 'Y',
        'ž' | 'ź' | 'ż' => 'z',
        'Ž' | 'Ź' | 'Ż' => 'Z',
        _ => c,
    }
}

// ---- element classes ------------------------------------------------------------------------------

const INLINE: &[&str] = &[
    "a", "abbr", "b", "bdi", "bdo", "cite", "code", "data", "dfn", "em", "i", "kbd", "label", "mark", "q", "s", "samp", "small",
    "span", "strong", "sub", "sup", "time", "u", "var", "wbr", "br", "font", "del", "ins", "big", "tt", "output", "nobr", "strike",
];

/// Elements that are never content.
const NEVER: &[&str] = &[
    "script", "style", "noscript", "template", "head", "meta", "link", "title", "base", "object", "param", "track", "source", "map",
    "area", "math", "frame", "frameset", "slot", "datalist", "#comment",
];

const HEADINGS: &[&str] = &["h1", "h2", "h3", "h4", "h5", "h6"];

fn heading_level(tag: &str) -> Option<u8> {
    match tag {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" => Some(4),
        "h5" => Some(5),
        "h6" => Some(6),
        _ => None,
    }
}

fn class_has(classes: &[&str], words: &[&str]) -> bool {
    classes.iter().any(|c| {
        let c = c.to_ascii_lowercase();
        words.iter().any(|w| c.contains(w))
    })
}

// ---- the reader -----------------------------------------------------------------------------------

pub struct Reader<'a> {
    dom: &'a Dom,
    st: &'a Styles,
    /// The page's URL (absolute), or "" for a local file without `--base`.
    base: String,
    /// The folder of a local page, for its relative images.
    local: Option<PathBuf>,
    /// Original id -> id in the clone (for `#anchor` links).
    anchors: HashMap<String, String>,
    dropped: BTreeMap<String, usize>,
    assets: Vec<(PathBuf, String)>,
    /// Per node: has a block-level element below it (precomputed).
    blocky: Vec<bool>,
    hidden: Vec<bool>,
    /// Per node: not drawn at all (hidden, but not only from screen readers: a thumbnail link
    /// with `aria-hidden` is still a picture on the screen).
    gone: Vec<bool>,
    nav: Option<usize>,
    foot: Option<usize>,
    used_images: HashSet<String>,
    /// Ids in the clone that belong to headings.
    heading_ids: HashSet<String>,
    /// An app's side navigation.
    side: Option<usize>,
    /// The motion of each element.
    mo: Motions,
    /// Features that need a server, in the order found.
    server: Vec<String>,
    /// Features whose `// TODO(server)` note is placed already.
    todo_placed: HashSet<String>,
    /// Boxes after a grid that were not part of it (read after it).
    leftover: Vec<usize>,
    videos: usize,
    title: String,
}

impl<'a> Reader<'a> {
    pub fn new(dom: &'a Dom, st: &'a Styles, base: &str, local: Option<PathBuf>, mo: Motions) -> Reader<'a> {
        let n = dom.nodes.len();
        let mut r = Reader {
            dom,
            st,
            base: base.to_string(),
            local,
            anchors: HashMap::new(),
            dropped: BTreeMap::new(),
            assets: Vec::new(),
            blocky: vec![false; n],
            hidden: vec![false; n],
            gone: vec![false; n],
            nav: None,
            foot: None,
            used_images: HashSet::new(),
            heading_ids: HashSet::new(),
            side: None,
            mo,
            server: Vec::new(),
            todo_placed: HashSet::new(),
            leftover: Vec::new(),
            videos: 0,
            title: String::new(),
        };
        // hidden: top-down (ids grow from parents to children)
        for id in 1..n {
            let p = dom.nodes[id].parent;
            r.hidden[id] = r.hidden[p] || (dom.is_elem(id) && r.hidden_self(id));
            r.gone[id] = r.gone[p] || (dom.is_elem(id) && r.hidden_look(id));
        }
        // blocky: bottom-up
        for id in (1..n).rev() {
            if !dom.is_elem(id) || r.hidden[id] {
                continue;
            }
            let own = !INLINE.contains(&dom.tag(id));
            let p = dom.nodes[id].parent;
            if own || r.blocky[id] {
                r.blocky[p] = true;
            }
        }
        r
    }

    fn hidden_self(&self, id: usize) -> bool {
        let d = self.dom;
        let aria = d.attr(id, "aria-hidden") == Some("true")
            && !(d.attr(id, "role") == Some("tabpanel") || d.classes(id).contains(&"tab-pane"));
        aria || self.hidden_look(id)
    }

    /// Not drawn: hidden for the eye (screen-reader-only text counts as not drawn too).
    fn hidden_look(&self, id: usize) -> bool {
        let d = self.dom;
        let t = d.tag(id);
        if NEVER.contains(&t) {
            return true;
        }
        // the panels of tabs are content even while another tab shows
        if d.attr(id, "role") == Some("tabpanel") || d.classes(id).contains(&"tab-pane") {
            return false;
        }
        // what the browser did not show (`--browser`)
        if d.attr(id, "data-nyra-hidden").is_some() {
            return true;
        }
        if d.attr(id, "hidden").is_some() || d.attr(id, "type") == Some("hidden") {
            return true;
        }
        if d.attr(id, "inert").is_some() {
            return true;
        }
        let classes = d.classes(id);
        if classes
            .iter()
            .any(|c| matches!(*c, "sr-only" | "visually-hidden" | "screen-reader-text" | "visuallyhidden" | "skip-link" | "skip" | "skip-to-content" | "skiplink"))
        {
            return true;
        }
        // a "Skip to content" link (the kit has its own)
        if t == "a"
            && d.attr(id, "href").is_some_and(|h| h.starts_with('#'))
            && d.nodes[id].kids.iter().any(|&k| d.tag(k) == "#text" && d.nodes[k].text.trim().to_lowercase().starts_with("skip to"))
        {
            return true;
        }
        if self.st.hidden(d, id) {
            return true;
        }
        // without the stylesheet: Tailwind's `hidden` with no breakpoint that shows it again
        if classes.contains(&"hidden")
            && !classes.iter().any(|c| c.contains(':') && (c.ends_with("block") || c.ends_with("flex") || c.ends_with("grid")))
        {
            return self.st.get(d, id, "display").is_none();
        }
        if let Some(style) = d.attr(id, "style") {
            let s = style.replace(' ', "").to_ascii_lowercase();
            if s.contains("display:none") || s.contains("visibility:hidden") {
                return true;
            }
        }
        false
    }

    fn count(&mut self, what: &str) {
        *self.dropped.entry(what.to_string()).or_default() += 1;
    }

    fn visible(&self, id: usize) -> bool {
        !self.hidden[id]
    }

    fn drawn(&self, id: usize) -> bool {
        !self.gone[id]
    }

    // ---- the whole page ----

    pub fn read(mut self) -> Page {
        let d = self.dom;
        let mut page = Page::default();
        let html = d.find(0, "html");
        page.lang = html
            .and_then(|h| d.attr(h, "lang"))
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .unwrap_or_else(|| "en".into());
        page.title = d.find(0, "title").map(|t| collapse(&d.nodes[t].text)).unwrap_or_default();
        let meta = |key: &str, val: &str| -> Option<String> {
            d.find_all(0, "meta")
                .into_iter()
                .find(|&m| d.attr(m, key).is_some_and(|v| v.eq_ignore_ascii_case(val)))
                .and_then(|m| d.attr(m, "content"))
                .map(collapse)
        };
        page.desc = meta("name", "description").or_else(|| meta("property", "og:description")).unwrap_or_default();
        let site_name = meta("property", "og:site_name");
        let theme_color = meta("name", "theme-color");

        // count what never becomes content
        for id in 0..d.nodes.len() {
            match d.tag(id) {
                "script" => {
                    if d.attr(id, "type").is_none_or(|t| !t.contains("json")) {
                        self.count("scripts");
                    }
                }
                "canvas" => self.count("canvas (drawn by scripts)"),
                "audio" => self.count("audio"),
                _ => {}
            }
        }

        let body = d.find(0, "body").unwrap_or(0);
        self.title = page.title.clone();
        self.side = self.find_side(body);
        self.nav = self.find_nav(body);
        self.foot = self.find_footer(body);
        let groups = {
            let mut g = Vec::new();
            self.split(body, &mut g);
            g
        };
        self.word_list(body, &mut page.words);
        page.images = self.all_images(body);

        // ids: the anchors the page links to, kept on the sections that hold them
        let targets: HashSet<String> =
            d.find_all(0, "a").into_iter().filter_map(|a| d.attr(a, "href")).filter_map(|h| self.fragment_of(h)).collect();
        let mut used: HashSet<String> = ["top", "main"].iter().map(|s| s.to_string()).collect();
        let mut section_ids = Vec::new();
        for g in &groups {
            let mut own = String::new();
            let mut inner: Vec<String> = Vec::new();
            for &n in g {
                for k in std::iter::once(n).chain(d.descendants(n)) {
                    // the kit's own ids: `#top` is the page, `#main` its content
                    if let Some(i) = d.attr(k, "id").filter(|i| *i != "top" && *i != "main") {
                        // a heading that links point to keeps its own id (its part may become a section)
                        if k != n && heading_level(d.tag(k)).is_some() && targets.contains(i) {
                            let hid = unique_id(&slug_id(i), &mut used);
                            self.anchors.insert(i.to_string(), hid.clone());
                            self.heading_ids.insert(hid);
                            continue;
                        }
                        if (k == n && own.is_empty() && g.len() == 1) || targets.contains(i) {
                            inner.push(i.to_string());
                        }
                    }
                }
                // an anchor right before the section (`<a name="x">` or an empty `<div id="x">`)
                if let Some(i) = d.attr(n, "name") {
                    inner.push(i.to_string());
                }
            }
            if let Some(first) = inner.iter().find(|i| targets.contains(*i)).or_else(|| inner.first()) {
                own = unique_id(&slug_id(first), &mut used);
            }
            for i in inner {
                if targets.contains(&i) && !own.is_empty() && !self.anchors.contains_key(&i) {
                    self.anchors.insert(i, own.clone());
                }
            }
            section_ids.push(own);
        }
        // anchors inside the footer go to the footer
        let mut foot_id = String::new();
        if let Some(f) = self.foot {
            for k in std::iter::once(f).chain(d.descendants(f)) {
                if let Some(i) = d.attr(k, "id") {
                    if targets.contains(i) && !self.anchors.contains_key(i) {
                        if foot_id.is_empty() {
                            foot_id = unique_id(&slug_id(i), &mut used);
                        }
                        self.anchors.insert(i.to_string(), foot_id.clone());
                    }
                }
            }
        }

        // the theme
        page.theme = self.theme(body, theme_color.as_deref());

        // the navbar, the side navigation, and what needs a server
        if let Some(n) = self.nav {
            page.nav = Some(self.read_nav(n, site_name.as_deref().unwrap_or("")));
        }
        if let Some(sd) = self.side {
            page.side = Some(self.read_side(sd));
        }
        page.todo_top = self.find_server(body);
        page.name = site_name
            .filter(|s| !s.is_empty())
            .or_else(|| page.nav.as_ref().map(|n| md_plain(&n.brand)).filter(|b| !b.is_empty() && words(b) <= 5))
            .unwrap_or_else(|| short_title(&page.title));

        // the sections
        let page_bg = page.theme.bg.as_deref().and_then(css::parse_color).unwrap_or(Rgb { r: 255.0, g: 255.0, b: 255.0, a: 1.0 });
        let primary = page.theme.primary.as_deref().and_then(css::parse_color);
        let mut has_h1 = false;
        for (k, g) in groups.iter().enumerate() {
            if page.sections.is_empty() {
                if let Some(panels) = self.read_split(g) {
                    page.sections.push(Section { kind: Kind::Split, panels, ..plain(section_ids[k].clone(), Vec::new()) });
                    continue;
                }
            }
            let mut items = Vec::new();
            for &n in g {
                self.flow_node(n, &mut items);
            }
            if items.is_empty() {
                continue;
            }
            let bg = if g.len() == 1 { self.section_bg(g[0], &page_bg, primary.as_ref()) } else { String::new() };
            let mut gmo = Some(self.group_motion(g));
            for mut s in self.sections_of(items, section_ids[k].clone(), &bg) {
                if let Some(m) = gmo.take() {
                    s.mo = m;
                }
                if s.level == 1 {
                    // the hero is the page's first part (after at most a short line, a notice bar)
                    let first = page.sections.iter().all(|x| x.title.is_empty() && x.kind == Kind::Plain && x.items.len() <= 2);
                    if !has_h1 && first {
                        let image = self.hero_image(g);
                        self.make_hero(&mut s, image);
                    }
                    has_h1 = true;
                }
                page.sections.push(s);
            }
        }
        // a first section with a heading and a big image is the hero even without an h1
        if !page.sections.iter().any(|s| s.kind == Kind::Hero) {
            if let Some(s) = page.sections.first_mut() {
                let has_img = s.items.iter().take(4).any(|i| matches!(i, Item::Img { .. }));
                let image = groups.first().and_then(|g| self.hero_image(g));
                if !s.title.is_empty() && (has_img || image.is_some()) {
                    self.make_hero(s, image);
                }
            }
        }
        // a hero's other content follows it as a plain section
        let mut out = Vec::new();
        for mut s in page.sections {
            if s.kind == Kind::Hero && !s.items.is_empty() {
                let rest = std::mem::take(&mut s.items);
                out.push(s);
                out.push(plain(String::new(), rest));
            } else {
                out.push(s);
            }
        }
        page.sections = out;

        if let Some(f) = self.foot {
            let mut foot = self.read_footer(f);
            foot.id = foot_id;
            page.footer = Some(foot);
        }
        self.place_search(&mut page);
        // the motion of what the clone shows
        for (n, what, word) in std::mem::take(&mut self.mo.notes) {
            if self.drawn(n) && !self.side.is_some_and(|sd| d.within(n, sd)) && !self.nav.is_some_and(|nv| d.within(n, nv)) {
                *page.motion.entry((what, word)).or_default() += 1;
            }
        }
        page.dropped = self.dropped;
        page.assets = self.assets;
        page.keyframes = self.mo.keyframes.clone();
        page.server = self.server;
        page.videos = self.videos;
        page
    }

    // ---- landmarks ----

    fn find_nav(&self, body: usize) -> Option<usize> {
        let d = self.dom;
        let all = d.descendants(body);
        let visible = |&n: &usize| self.visible(n) && !self.side.is_some_and(|s| d.within(n, s));
        let header = all.iter().copied().filter(visible).find(|&n| {
            (d.tag(n) == "header" || d.attr(n, "role") == Some("banner")) && !self.inside(n, &["article", "main", "section", "footer"])
        });
        let nav = all
            .iter()
            .copied()
            .filter(visible)
            .find(|&n| (d.tag(n) == "nav" || d.attr(n, "role") == Some("navigation")) && !self.inside(n, &["footer"]));
        // a header that also holds the hero (an h1 and its text) is not only a navbar
        // (a header with the page's h1 and no links is its hero, not a bar)
        let bar = |h: usize| {
            let h1 = d.find(h, "h1").filter(|&x| self.visible(x));
            (h1.is_none_or(|h1| nav.is_some_and(|n| d.within(h1, n))) || self.count_words(h) < 30)
                && !(h1.is_some() && d.find_all(h, "a").into_iter().filter(|&a| self.visible(a)).count() < 2)
        };
        match (header, nav) {
            (Some(h), Some(n)) if d.within(n, h) && !bar(h) => Some(n),
            (Some(h), _) if bar(h) => Some(h),
            (_, Some(n)) => {
                // the nav's box at the top of the page: its parent when that holds the brand too
                let p = d.nodes[n].parent;
                if p != body && d.elems(p).len() <= 4 && d.find(p, "h1").is_none() && self.count_words(p) < 80 {
                    Some(p)
                } else {
                    Some(n)
                }
            }
            _ => all.iter().copied().filter(visible).take(40).find(|&n| {
                let c = d.classes(n);
                (d.tag(n) == "div" || d.tag(n) == "section")
                    && class_has(&c, &["navbar", "site-header", "masthead", "topbar", "top-bar", "main-nav", "menu-bar"])
            }),
        }
    }

    fn find_footer(&self, body: usize) -> Option<usize> {
        let d = self.dom;
        let all = d.descendants(body);
        let found = all.iter().rev().copied().filter(|&n| self.visible(n)).find(|&n| {
            (d.tag(n) == "footer" || d.attr(n, "role") == Some("contentinfo"))
                && !self.inside(n, &["article", "blockquote", "figure", "form"])
        });
        found.or_else(|| {
            let kids = d.elems(body);
            kids.iter()
                .rev()
                .take(3)
                .copied()
                .find(|&n| class_has(&d.classes(n), &["footer"]) || d.attr(n, "id").is_some_and(|i| i.contains("footer")))
        })
    }

    /// True if an ancestor of `id` has one of these tags.
    fn inside(&self, id: usize, tags: &[&str]) -> bool {
        let mut n = self.dom.nodes[id].parent;
        while n != 0 {
            if tags.contains(&self.dom.tag(n)) {
                return true;
            }
            n = self.dom.nodes[n].parent;
        }
        false
    }

    fn is_banner(&self, id: usize) -> bool {
        let d = self.dom;
        let mut names: Vec<String> = d.classes(id).iter().map(|c| c.to_ascii_lowercase()).collect();
        if let Some(i) = d.attr(id, "id") {
            names.push(i.to_ascii_lowercase());
        }
        let cookie = names.iter().any(|c| {
            c.contains("cookie") || c.contains("consent") || c.contains("gdpr") || c.starts_with("cmp") || c.contains("onetrust")
        });
        let modal = d.attr(id, "role") == Some("dialog")
            || d.attr(id, "aria-modal") == Some("true")
            || d.tag(id) == "dialog"
            || names.iter().any(|c| c == "modal" || c.starts_with("modal-") || c.contains("popup") || c.contains("lightbox"));
        cookie || modal
    }

    // ---- sections ----

    /// The groups of top-level elements that make the page's sections, in order.
    fn split(&mut self, id: usize, out: &mut Vec<Vec<usize>>) {
        let d = self.dom;
        let mut current: Option<Vec<usize>> = None;
        for k in d.elems(id) {
            if !self.visible(k) || Some(k) == self.nav || Some(k) == self.foot || Some(k) == self.side {
                continue;
            }
            if self.is_banner(k) {
                self.count("cookie banners and dialogs");
                continue;
            }
            let holds_nav = self.nav.is_some_and(|n| d.within(n, k)) || self.side.is_some_and(|n| d.within(n, k));
            let holds_foot = self.foot.is_some_and(|f| d.within(f, k));
            // a split screen at the top of the page stays one group (its panels are read together)
            if out.is_empty() && current.is_none() && !holds_nav && !holds_foot && self.split_panels(k).is_some() {
                out.push(vec![k]);
                continue;
            }
            if holds_nav || holds_foot || self.is_wrapper(k) {
                if let Some(c) = current.take() {
                    out.push(c);
                }
                self.split(k, out);
                continue;
            }
            if !self.has_content(k) {
                // an empty anchor box before a section: its id travels with the next group
                if d.attr(k, "id").is_some() {
                    current.get_or_insert_with(Vec::new).push(k);
                }
                continue;
            }
            if self.section_like(k) || heading_level(d.tag(k)).is_some_and(|l| l <= 3) {
                if let Some(c) = current.take() {
                    // a lone anchor box joins the section it marks
                    if c.iter().all(|&n| !self.has_content(n)) {
                        let mut c = c;
                        c.push(k);
                        current = Some(c);
                        continue;
                    }
                    out.push(c);
                }
                if self.section_like(k) {
                    out.push(vec![k]);
                } else {
                    current = Some(vec![k]);
                }
            } else {
                current.get_or_insert_with(Vec::new).push(k);
            }
        }
        if let Some(c) = current {
            if c.iter().any(|&n| self.has_content(n)) {
                out.push(c);
            }
        }
    }

    /// A box around sections (or a box around one box): read its children instead.
    fn is_wrapper(&self, id: usize) -> bool {
        let d = self.dom;
        if !matches!(d.tag(id), "div" | "main" | "span" | "article" | "body" | "center" | "form") || d.tag(id) == "form" {
            return false;
        }
        let kids: Vec<usize> = d.elems(id).into_iter().filter(|&k| self.visible(k) && self.has_content(k)).collect();
        let direct_text = d.nodes[id].kids.iter().any(|&k| d.tag(k) == "#text" && !d.nodes[k].text.trim().is_empty());
        if direct_text {
            return false;
        }
        if kids.len() == 1 {
            return !matches!(d.tag(kids[0]), "p" | "img" | "ul" | "ol" | "table" | "blockquote" | "form")
                && heading_level(d.tag(kids[0])).is_none();
        }
        let sections = kids.iter().filter(|&&k| self.section_like(k)).count();
        let direct_heading = kids.iter().any(|&k| heading_level(d.tag(k)).is_some());
        sections >= 2 && !direct_heading && (sections * 2 >= kids.len() || d.tag(id) == "main")
    }

    /// An element that is a section by itself.
    fn section_like(&self, id: usize) -> bool {
        let d = self.dom;
        match d.tag(id) {
            "section" | "article" | "header" | "aside" => true,
            "div" | "main" | "figure" | "form" | "button" | "a" => {
                let has_heading = HEADINGS.iter().any(|h| d.find(id, h).is_some_and(|n| self.visible(n)));
                has_heading && self.count_words(id) >= 3 && d.elems(id).len() >= 2 || d.tag(id) == "form"
            }
            _ => false,
        }
    }

    fn has_content(&self, id: usize) -> bool {
        if !self.visible(id) {
            return false;
        }
        if self.count_words(id) > 0 {
            return true;
        }
        let d = self.dom;
        matches!(d.tag(id), "img" | "picture" | "video" | "iframe" | "form")
            || d.descendants(id).into_iter().any(|k| self.visible(k) && matches!(d.tag(k), "img" | "picture" | "iframe" | "form"))
            || self.st.bg_image(d, id).is_some()
    }

    fn count_words(&self, id: usize) -> usize {
        let d = self.dom;
        if d.tag(id) == "#text" {
            return words(&d.nodes[id].text);
        }
        if !self.visible(id) {
            return 0;
        }
        let n = d.nodes[id].kids.iter().map(|&k| self.count_words(k)).sum();
        if n == 0 && d.tag(id) != "nav" {
            // letters hidden from screen readers under a label that reads them (`aria-label`)
            if let Some(l) = d.attr(id, "aria-label") {
                return words(l);
            }
        }
        n
    }

    fn word_list(&self, id: usize, out: &mut Vec<String>) {
        let d = self.dom;
        if d.tag(id) == "#text" {
            out.extend(d.nodes[id].text.split_whitespace().filter(|w| w.chars().any(char::is_alphanumeric)).map(str::to_string));
            return;
        }
        if !self.visible(id) {
            return;
        }
        let before = out.len();
        for &k in &d.nodes[id].kids {
            self.word_list(k, out);
        }
        if out.len() == before && d.tag(id) != "nav" {
            if let Some(l) = d.attr(id, "aria-label") {
                out.extend(l.split_whitespace().map(str::to_string));
            }
        }
    }

    fn text_of(&self, id: usize) -> String {
        let d = self.dom;
        if d.tag(id) == "#text" {
            return d.nodes[id].text.clone();
        }
        if !self.visible(id) {
            return String::new();
        }
        let mut s = String::new();
        for &k in &d.nodes[id].kids {
            let t = self.text_of(k);
            if !INLINE.contains(&d.tag(k)) && d.tag(k) != "#text" {
                s.push(' ');
                s.push_str(&t);
            } else if d.tag(k) != "#text" {
                s.push_str(&self.spaced(k, t));
            } else {
                s.push_str(&t);
            }
        }
        if s.trim().is_empty() {
            if let Some(l) = d.attr(id, "aria-label") {
                return l.to_string();
            }
        }
        s
    }

    fn all_images(&self, body: usize) -> Vec<String> {
        let d = self.dom;
        let mut v = Vec::new();
        for k in d.descendants(body) {
            if d.tag(k) == "img" && self.drawn(k) && !self.is_icon(k) {
                if let Some(u) = self.img_url(k) {
                    if !v.contains(&u) {
                        v.push(u);
                    }
                }
            }
        }
        v
    }

    /// Splits a group's items into sections at its top headings, and takes each one's title,
    /// kicker and lead text.
    fn sections_of(&mut self, items: Vec<Item>, id: String, bg: &str) -> Vec<Section> {
        // the highest heading level that repeats: a page's one h1 then its h2 parts
        // (h1 and h2 together: a page's parts may be h2 here and h1 there; h3 only without them)
        let count_of = |lv: u8| items.iter().filter(|i| matches!(i, Item::H(l, _, _) if *l <= lv)).count();
        let top = if count_of(2) >= 2 {
            Some(2)
        } else if count_of(2) == 0 && count_of(3) >= 2 {
            Some(3)
        } else {
            None
        };
        let mut chunks: Vec<Vec<Item>> = Vec::new();
        if let Some(lv) = top {
            {
                let mut cur = Vec::new();
                for it in items {
                    let starts = matches!(&it, Item::H(l, _, _) if *l <= lv);
                    if starts && cur.iter().any(|i| !is_kicker(i)) {
                        // a short line right before the heading is its kicker
                        // (not the only line under the previous heading: that is its text)
                        let carry = if cur.len() >= 3
                            && cur.last().is_some_and(|i| matches!(i, Item::P(t) if words(&md_plain(t)) <= 4 && !t.ends_with('.')))
                        {
                            cur.pop()
                        } else {
                            None
                        };
                        // a note for the program right before the heading belongs to its part
                        let mut notes = Vec::new();
                        while matches!(cur.last(), Some(Item::Todo(_))) {
                            notes.insert(0, cur.pop().unwrap());
                        }
                        chunks.push(std::mem::take(&mut cur));
                        cur.extend(notes);
                        if let Some(c) = carry {
                            cur.push(c);
                        }
                    }
                    cur.push(it);
                }
                chunks.push(cur);
            }
        } else {
            chunks.push(items);
        }
        let mut out = Vec::new();
        for (k, chunk) in chunks.into_iter().enumerate() {
            let mut s = plain(if k == 0 { id.clone() } else { String::new() }, chunk);
            if k == 0 {
                s.bg = bg.to_string();
            }
            take_head(&mut s);
            if !s.items.is_empty() || !s.title.is_empty() || !s.text.is_empty() {
                out.push(s);
            }
        }
        out
    }

    fn make_hero(&mut self, s: &mut Section, image: Option<(String, String)>) {
        s.kind = Kind::Hero;
        // the first image of the section is the hero's photo, unless the background is one
        if let Some((src, alt)) = image {
            s.image = src;
            s.alt = alt;
        } else if let Some(pos) = s.items.iter().take(6).position(|i| matches!(i, Item::Img { .. })) {
            if let Item::Img { src, alt, px, mo, width, .. } = s.items.remove(pos) {
                // a picture the browser drew narrower than half the window (or a small one) stands
                // beside the text
                if (px > 0 && px < 720) || (px == 0 && !width.is_empty()) {
                    s.aside = Some((src, alt, mo));
                } else {
                    s.image = src;
                    s.alt = alt;
                }
            }
        }
        // the lead text, if `take_head` left it, and the buttons
        if s.text.is_empty() {
            if let Some(Item::P(t)) = s.items.first() {
                if words(t) <= 80 {
                    s.text = t.clone();
                    s.items.remove(0);
                }
            }
        }
        if let Some(pos) = s.items.iter().take(3).position(|i| matches!(i, Item::Buttons(_))) {
            if let Item::Buttons(b) = s.items.remove(pos) {
                s.buttons = b.into_iter().take(3).collect();
            }
        }
        if s.title.is_empty() {
            s.kind = Kind::Plain;
        }
    }

    /// The hero's photo: a background image of the section's box (or of a box inside it), else
    /// a large image in it.
    fn hero_image(&mut self, g: &[usize]) -> Option<(String, String)> {
        let d = self.dom;
        for &n in g {
            let mut boxes = vec![n];
            boxes.extend(
                d.descendants(n)
                    .into_iter()
                    .filter(|&k| self.visible(k) && matches!(d.tag(k), "div" | "section" | "figure" | "picture" | "span"))
                    .take(12),
            );
            for b in boxes {
                if let Some(u) = self.st.bg_image(d, b) {
                    if let Some(src) = self.asset(&u) {
                        return Some((src, String::new()));
                    }
                }
            }
        }
        None
    }

    /// The first section as a choice of two or more panels, each with a title and a photo
    /// (a split screen of subsites).
    fn read_split(&mut self, g: &[usize]) -> Option<Vec<Card>> {
        if g.len() != 1 {
            return None;
        }
        let (panels, others) = self.split_panels(g[0])?;
        if others > 0 {
            *self.dropped.entry("parts of a split screen between its panels".into()).or_default() += others;
        }
        {
            let mut cards = Vec::new();
            for p in panels {
                let mut items = Vec::new();
                self.flow_children(p, &mut items);
                let mut c = self.card_from(p, items);
                if c.image.is_empty() {
                    if let Some((src, _)) = self.hero_image(&[p]) {
                        c.image = src;
                    }
                }
                // each panel's own color (the tint over its photo)
                let d = self.dom;
                c.color = std::iter::once(p)
                    .chain(d.descendants(p))
                    .take(40)
                    .find_map(|k| self.st.tint(d, k))
                    .map(|c| c.hex())
                    .unwrap_or_default();
                cards.push(c);
            }
            Some(cards)
        }
    }

    /// The panels of a split screen below `n` (each with an h1 or h2 and a photo, two to four
    /// side by side), and how many other boxes sit between them.
    fn split_panels(&self, n: usize) -> Option<(Vec<usize>, usize)> {
        let d = self.dom;
        let mut n = n;
        for _ in 0..8 {
            let kids: Vec<usize> = d.elems(n).into_iter().filter(|&k| self.visible(k) && self.has_content(k)).collect();
            if kids.len() == 1 {
                n = kids[0];
                continue;
            }
            let panels: Vec<usize> = kids
                .iter()
                .copied()
                .filter(|&p| {
                    d.descendants(p).into_iter().any(|k| self.visible(k) && matches!(d.tag(k), "h1" | "h2"))
                        && d.descendants(p)
                            .into_iter()
                            .any(|k| self.visible(k) && (d.tag(k) == "img" || self.st.bg_image(d, k).is_some()))
                        && !self.section_like_big(p)
                })
                .collect();
            let ok =
                (2..=4).contains(&panels.len()) && panels.len() * 2 >= kids.len() && panels.iter().any(|&p| d.find(p, "h1").is_some());
            return ok.then(|| (panels.clone(), kids.len() - panels.len()));
        }
        None
    }

    fn section_bg(&self, id: usize, page: &Rgb, primary: Option<&Rgb>) -> String {
        let d = self.dom;
        let mut n = id;
        let mut c = self.st.bg(d, n);
        // a section's color is often on its first inner box
        for _ in 0..2 {
            if c.is_some() {
                break;
            }
            let kids = d.elems(n);
            if kids.len() != 1 {
                break;
            }
            n = kids[0];
            c = self.st.bg(d, n);
        }
        let Some(c) = c else { return String::new() };
        if c.distance(page) < 14.0 {
            return String::new();
        }
        if primary.is_some_and(|p| c.distance(p) < 40.0) {
            return "primary".into();
        }
        let dark_page = page.luma() < 0.2;
        if c.luma() < 0.12 && !dark_page {
            return "dark".into();
        }
        if !dark_page && c.luma() > 0.97 {
            return "white".into();
        }
        "alt".into()
    }

    // ---- the flow of content ----

    fn flow_node(&mut self, id: usize, out: &mut Vec<Item>) {
        let d = self.dom;
        if d.tag(id) == "#text" {
            let t = collapse(&md_escape(&d.nodes[id].text));
            if !t.is_empty() {
                out.push(Item::P(t));
            }
            return;
        }
        if !self.visible(id) {
            return;
        }
        if INLINE.contains(&d.tag(id)) && !self.blocky[id] {
            let md = collapse(&self.inline_md(id));
            self.push_text(md, id, out);
            return;
        }
        self.block(id, out);
    }

    /// The children of `id` as items: runs of inline content become paragraphs.
    fn flow_children(&mut self, id: usize, out: &mut Vec<Item>) {
        let d = self.dom;
        let mut run = String::new();
        let mut run_links: Vec<usize> = Vec::new();
        let mut only_links = true;
        for &k in &d.nodes[id].kids {
            let t = d.tag(k);
            if t == "#text" {
                if !d.nodes[k].text.trim().is_empty() {
                    only_links = false;
                }
                run.push_str(&md_escape(&d.nodes[k].text));
                continue;
            }
            if !self.visible(k) {
                continue;
            }
            if INLINE.contains(&t) && !self.blocky[k] {
                if t == "a" {
                    run_links.push(k);
                } else if self.count_words(k) > 0 {
                    only_links = false;
                }
                run.push_str(&self.inline_node(k));
                continue;
            }
            self.flush(&mut run, &mut run_links, &mut only_links, out);
            self.block(k, out);
        }
        self.flush(&mut run, &mut run_links, &mut only_links, out);
    }

    fn flush(&mut self, run: &mut String, links: &mut Vec<usize>, only_links: &mut bool, out: &mut Vec<Item>) {
        let md = collapse(run);
        if !md.is_empty() {
            // a run of links that look like buttons is a row of buttons
            // (two or more links alone in a row are buttons when one of them looks like one)
            let row = *only_links && links.len() >= 2 && links.len() <= 4 && links.iter().any(|&a| self.looks_like_button(a));
            let buttons: Vec<(String, String)> =
                if *only_links && !links.is_empty() && (row || links.iter().all(|&a| self.looks_like_button(a))) {
                    links.iter().filter_map(|&a| self.button_of(a)).collect()
                } else {
                    Vec::new()
                };
            if !buttons.is_empty() && buttons.len() == links.len() {
                out.push(Item::Buttons(buttons));
            } else {
                out.push(Item::P(md));
            }
        }
        run.clear();
        links.clear();
        *only_links = true;
    }

    fn push_text(&mut self, md: String, id: usize, out: &mut Vec<Item>) {
        if md.is_empty() {
            return;
        }
        if self.dom.tag(id) == "a" && self.looks_like_button(id) {
            if let Some(b) = self.button_of(id) {
                if let Some(Item::Buttons(v)) = out.last_mut() {
                    v.push(b);
                } else {
                    out.push(Item::Buttons(vec![b]));
                }
                return;
            }
        }
        out.push(Item::P(md));
    }

    fn block(&mut self, id: usize, out: &mut Vec<Item>) {
        let d = self.dom;
        let t = d.tag(id).to_string();
        if self.is_banner(id) {
            self.count("cookie banners and dialogs");
            return;
        }
        if let Some(feature) = self.server_box(id) {
            self.todo(feature, out);
        }
        if self.is_player(id) {
            match self.video_item(id) {
                Some(v) => out.push(v),
                None => self.count("videos streamed by scripts"),
            }
            return;
        }
        if let Some(level) = heading_level(&t) {
            let md = collapse(&self.inline_md(id));
            if !md.is_empty() {
                let hid = d
                    .attr(id, "id")
                    .and_then(|i| self.anchors.get(i))
                    .filter(|h| self.heading_ids.contains(*h))
                    .cloned()
                    .unwrap_or_default();
                out.push(Item::H(level, md, hid));
            }
            // an image inside a heading (a logo)
            for img in d.find_all(id, "img") {
                self.push_img(img, out);
            }
            return;
        }
        match t.as_str() {
            "p" => self.flow_children(id, out),
            "img" | "picture" => self.push_img(id, out),
            "video" => match self.video_item(id) {
                Some(v) => out.push(v),
                None => {
                    let poster = d.attr(id, "poster").map(|p| css::resolve(&self.base, p));
                    match poster.and_then(|p| self.asset(&p)) {
                        Some(src) => out.push(Item::Img { src, alt: String::new(), caption: String::new(), mo: Mo::default(), width: String::new(), px: 0 }),
                        None => self.count("videos streamed by scripts"),
                    }
                }
            },
            "marquee" => {
                let words: Vec<String> = self.lines_of(id);
                if !words.is_empty() {
                    out.push(Item::Marquee(words));
                }
            }
            "iframe" => self.iframe(id, out),
            "svg" => self.count("inline SVG graphics"),
            "ul" | "ol" | "menu" => self.list(id, t == "ol", out),
            "table" => self.table(id, out),
            "dl" => self.dl(id, out),
            "blockquote" => {
                let q = self.quote_of(id);
                if !q.text.is_empty() {
                    out.push(Item::Quotes(vec![q]));
                }
            }
            "figure" => {
                if let Some(bq) = d.find(id, "blockquote") {
                    let mut q = self.quote_of(bq);
                    if q.name.is_empty() {
                        if let Some(fc) = d.find(id, "figcaption") {
                            q.name = collapse(&self.inline_md(fc)).trim_start_matches(['—', '–', '-', ' ']).to_string();
                        }
                    }
                    split_name(&mut q);
                    out.push(Item::Quotes(vec![q]));
                } else {
                    let cap = d.find(id, "figcaption").map(|fc| collapse(&self.inline_md(fc))).unwrap_or_default();
                    let imgs: Vec<usize> = d.descendants(id).into_iter().filter(|&k| d.tag(k) == "img" && self.visible(k)).collect();
                    if imgs.len() == 1 {
                        let before = out.len();
                        self.push_img(imgs[0], out);
                        if let Some(Item::Img { caption, .. }) = out.get_mut(before) {
                            *caption = cap;
                        } else if !cap.is_empty() {
                            out.push(Item::P(cap));
                        }
                    } else {
                        self.container(id, out);
                    }
                }
            }
            "form" => {
                if d.descendants(id).into_iter().any(|k| d.tag(k) == "input" && d.attr(k, "type") == Some("file")) {
                    self.todo("uploads", out);
                }
                if let Some(f) = self.form(id) {
                    out.push(Item::Form(f));
                } else if let Some(i) = d.descendants(id).into_iter().find(|&k| self.visible(k) && d.tag(k) == "input" && self.searchish(k)) {
                    // a search form of this part of the page: the clone filters what follows it
                    let hint = d.attr(i, "placeholder").or_else(|| d.attr(i, "aria-label")).map(collapse).unwrap_or_default();
                    out.push(Item::Search(hint));
                }
            }
            "address" => {
                let lines = self.lines_of(id);
                if !lines.is_empty() {
                    out.push(Item::Address(lines));
                }
            }
            "pre" => {
                let code = d.nodes[id].kids.iter().map(|&k| self.raw_text(k)).collect::<String>();
                let code = code.trim_matches('\n').to_string();
                if !code.trim().is_empty() {
                    out.push(Item::Code(code));
                }
            }
            "hr" | "br" => {}
            "details" => {
                if let Some(s) = d.elems(id).into_iter().find(|&k| d.tag(k) == "summary") {
                    let md = collapse(&self.inline_md(s));
                    if !md.is_empty() {
                        out.push(Item::H(4, md, String::new()));
                    }
                }
                for k in d.elems(id).into_iter().filter(|&k| d.tag(k) != "summary") {
                    self.flow_node(k, out);
                }
            }
            "button" => {
                // an action of the page's scripts (Subscribe, Share, Save): a static button that
                // goes to the original page, when it has a short label
                let label = collapse(&d.nodes[id].kids.iter().map(|&k| self.drawn_text(k)).collect::<Vec<_>>().join(" "));
                let generic = ["menu", "close", "next", "previous", "prev", "toggle", "search", "more", "less", "×", "x", "ok", "play", "pause", "mute"];
                let page_url = (self.base.starts_with("http")).then(|| self.base.clone());
                if !label.is_empty() && words(&label) <= 3 && label.chars().count() <= 24 && !generic.contains(&label.to_lowercase().as_str()) && page_url.is_some() {
                    let b = (md_escape(&label), page_url.unwrap());
                    if let Some(Item::Buttons(v)) = out.last_mut() {
                        v.push(b);
                    } else {
                        out.push(Item::Buttons(vec![b]));
                    }
                    self.count("buttons that run scripts (kept as links to the original page)");
                } else if self.blocky[id] {
                    self.container(id, out);
                } else if self.count_words(id) > 0 {
                    self.count("buttons that run scripts");
                }
            }
            "input" if self.searchish(id) => {
                let hint = d.attr(id, "placeholder").or_else(|| d.attr(id, "aria-label")).map(collapse).unwrap_or_default();
                out.push(Item::Search(hint));
            }
            "select" | "input" | "textarea" | "option" => self.count("form controls outside a form"),
            "dialog" => self.count("cookie banners and dialogs"),
            _ => self.container(id, out),
        }
    }

    /// A box: a repeated group (cards, a gallery, stats, quotes, steps) or its children in order.
    fn container(&mut self, id: usize, out: &mut Vec<Item>) {
        if self.tabs(id, out) {
            return;
        }
        if self.dom.attr(id, "role") == Some("tablist") {
            if let Some(item) = self.tab_links(id) {
                out.push(item);
                return;
            }
        }
        if let Some(item) = self.carousel(id) {
            out.push(item);
            return;
        }
        if self.mo.get(id).looping == "marquee" {
            if let Some(item) = self.marquee_of(id) {
                out.push(item);
                return;
            }
        }
        if let Some(item) = self.group(id) {
            if let Item::Media { items, .. } = &item {
                let priced = items.iter().any(|m| !m.price.is_empty());
                if priced && self.server.iter().any(|f| f == "a cart and payments") {
                    self.todo("a cart and payments", out);
                }
            }
            let item = self.moved(id, item);
            out.push(item);
            for k in std::mem::take(&mut self.leftover) {
                self.flow_node(k, out);
            }
            return;
        }
        // a box that is a link around content (a card-like teaser): its content, and the link
        self.flow_children(id, out);
    }

    // ---- inline text ----

    fn inline_md(&mut self, id: usize) -> String {
        let d = self.dom;
        let mut s = String::new();
        // the children of a flex or grid box are boxes of their own, apart from each other
        let boxes = self.st.get(d, id, "display").is_some_and(|v| v.contains("flex") || v.contains("grid"));
        for &k in &d.nodes[id].kids {
            let part = self.inline_node(k);
            if boxes && !part.trim().is_empty() {
                s.push(' ');
            }
            s.push_str(&part);
        }
        if s.trim().is_empty() {
            // an icon-only link or a label read by screen readers
            if let Some(l) = d.attr(id, "aria-label").or_else(|| d.attr(id, "title")) {
                return md_escape(l);
            }
        }
        s
    }

    fn inline_node(&mut self, k: usize) -> String {
        let d = self.dom;
        let t = d.tag(k);
        if t == "#text" {
            return md_escape(&d.nodes[k].text);
        }
        if !self.visible(k) {
            // `<span aria-label="Flora"><span aria-hidden>F</span>...`: the label stands for the letters
            return String::new();
        }
        match t {
            "br" => " ".into(),
            "img" | "svg" | "picture" | "video" | "iframe" | "canvas" | "input" | "select" | "textarea" | "button" => String::new(),
            "a" => {
                let mut inner = collapse(&self.inline_md(k));
                if inner.is_empty() {
                    if let Some(img) = d.find(k, "img") {
                        inner = md_escape(&collapse(d.attr(img, "alt").unwrap_or("")));
                    }
                }
                if inner.is_empty() {
                    return String::new();
                }
                match d.attr(k, "href").and_then(|h| self.href(h)) {
                    Some(u) => self.spaced(k, format!("[{inner}]({})", md_url(&u))),
                    None => self.spaced(k, inner),
                }
            }
            "strong" | "b" => {
                let inner = self.inline_md(k);
                self.spaced(k, wrap(&inner, "**"))
            }
            "em" | "i" | "cite" | "dfn" => {
                let inner = self.inline_md(k);
                self.spaced(k, wrap(&inner, "*"))
            }
            "code" | "kbd" | "samp" | "tt" => {
                let raw = collapse(&self.raw_text(k)).replace('`', "'");
                if raw.is_empty() {
                    String::new()
                } else {
                    format!("`{raw}`")
                }
            }
            _ => {
                let inner = self.inline_md(k);
                if INLINE.contains(&t) {
                    self.spaced(k, inner)
                } else {
                    format!(" {inner} ")
                }
            }
        }
    }

    /// An inline element that the page's CSS shows as a box (`display: block`, `flex`, ...) is
    /// apart from its neighbors: `<span>v0.7</span><span>New</span>` reads "v0.7 New".
    fn spaced(&self, k: usize, s: String) -> String {
        let boxy = self.st.get(self.dom, k, "display").is_some_and(|d| {
            let d = d.trim();
            d.starts_with("block")
                || d.contains("flex")
                || d.contains("grid")
                || d == "inline-block"
                || d == "table"
                || d == "list-item"
        });
        if boxy {
            format!(" {s} ")
        } else {
            s
        }
    }

    /// The text of a subtree as it is written (for code).
    fn raw_text(&self, k: usize) -> String {
        let d = self.dom;
        if d.tag(k) == "#text" {
            return d.nodes[k].text.clone();
        }
        if !self.visible(k) {
            return String::new();
        }
        if d.tag(k) == "br" {
            return "\n".into();
        }
        d.nodes[k].kids.iter().map(|&c| self.raw_text(c)).collect()
    }

    /// Lines of an element split at `<br>` and block children (an address, a footer column).
    fn lines_of(&mut self, id: usize) -> Vec<String> {
        let d = self.dom;
        let mut lines = Vec::new();
        let mut cur = String::new();
        let kids = d.nodes[id].kids.clone();
        for k in kids {
            let t = d.tag(k).to_string();
            if t == "br" || (!INLINE.contains(&t.as_str()) && t != "#text") {
                let l = collapse(&cur);
                if !l.is_empty() {
                    lines.push(l);
                }
                cur.clear();
                if t != "br" && self.visible(k) {
                    if self.blocky[k] {
                        lines.extend(self.lines_of(k));
                    } else {
                        let l = collapse(&self.inline_md(k));
                        if !l.is_empty() {
                            lines.push(l);
                        }
                    }
                }
                continue;
            }
            cur.push_str(&self.inline_node(k));
        }
        let l = collapse(&cur);
        if !l.is_empty() {
            lines.push(l);
        }
        lines
    }

    // ---- links and images ----

    /// The fragment of a link to a part of this page (`#x`, or this page's URL with `#x`).
    fn fragment_of(&self, href: &str) -> Option<String> {
        let h = href.trim();
        if let Some(f) = h.strip_prefix('#') {
            return (!f.is_empty()).then(|| percent_decode(f));
        }
        let at = h.find('#')?;
        let abs = css::resolve(&self.base, &h[..at]);
        let here = self.base.split('#').next().unwrap_or("");
        let same =
            !self.base.is_empty() && (abs == here || abs.trim_end_matches('/') == here.trim_end_matches('/') || h[..at].is_empty());
        (same && at + 1 < h.len()).then(|| percent_decode(&h[at + 1..]))
    }

    /// A link target for the clone: absolute, an anchor that exists, `mailto:` or `tel:`; `None`
    /// for one the clone cannot keep (a script, an anchor to something left out).
    fn href(&self, raw: &str) -> Option<String> {
        let h = raw.trim();
        if h.is_empty() || h == "#" {
            return None;
        }
        if let Some(f) = self.fragment_of(h) {
            if f == "top" {
                return Some("#top".into());
            }
            return self.anchors.get(&f).map(|i| format!("#{i}"));
        }
        let lower = h.to_ascii_lowercase();
        if lower.starts_with("mailto:") || lower.starts_with("tel:") {
            return Some(h.to_string());
        }
        if css::has_scheme(h) {
            return (lower.starts_with("http://") || lower.starts_with("https://")).then(|| h.to_string());
        }
        if self.base.is_empty() {
            // a local page without --base: its own links stay relative
            return Some(h.to_string());
        }
        Some(css::resolve(&self.base, h))
    }

    fn looks_like_button(&self, a: usize) -> bool {
        let d = self.dom;
        let c = d.classes(a);
        if class_has(&c, &["btn", "button", "cta"]) || d.attr(a, "role") == Some("button") || d.attr(a, "data-nyra-btn").is_some() {
            return true;
        }
        // a link with its own background color and a short label
        self.st.bg(d, a).is_some() && self.count_words(a) <= 5 && self.count_words(a) > 0
    }

    fn button_of(&self, a: usize) -> Option<(String, String)> {
        let d = self.dom;
        let label = collapse(&md_escape(&collapse(&self.text_of(a))));
        let href = d.attr(a, "href").and_then(|h| self.href(h))?;
        (!label.is_empty()).then_some((label, href))
    }

    fn is_icon(&self, img: usize) -> bool {
        let d = self.dom;
        // the size the browser drew it at (`--browser`)
        if let Some(w) = d.attr(img, "data-nyra-w").and_then(|w| w.parse::<f64>().ok()) {
            if w > 0.0 && w <= 64.0 {
                return true;
            }
        }
        let size = |k: &str| d.attr(img, k).and_then(|v| v.trim_end_matches("px").parse::<f64>().ok());
        let small = |v: Option<f64>| v.is_some_and(|v| v <= 64.0);
        if small(size("width")) && (small(size("height")) || size("height").is_none())
            || small(size("height")) && size("width").is_none()
        {
            return true;
        }
        let c = d.classes(img);
        if class_has(&c, &["icon", "emoji", "avatar-xs", "flag"]) {
            return true;
        }
        let src = d.attr(img, "src").unwrap_or("").to_ascii_lowercase();
        src.contains("/icons/") || src.ends_with(".ico") || src.contains("emoji") || src.contains("pixel") && src.contains("track")
    }

    /// The URL of an image (the largest candidate of its srcset), absolute.
    fn img_url(&self, img: usize) -> Option<String> {
        let d = self.dom;
        let img = if d.tag(img) == "picture" { d.find(img, "img").unwrap_or(img) } else { img };
        let mut cands: Vec<String> = Vec::new();
        for k in ["data-src", "data-lazy-src", "data-original", "data-lazy", "src"] {
            if let Some(v) = d.attr(img, k) {
                let v = v.trim();
                if !v.is_empty() && !v.starts_with("data:") {
                    cands.push(v.to_string());
                }
            }
        }
        let mut srcsets: Vec<&str> = Vec::new();
        for k in ["data-srcset", "srcset"] {
            if let Some(v) = d.attr(img, k) {
                srcsets.push(v);
            }
        }
        // <picture><source srcset>
        let p = d.nodes[img].parent;
        if d.tag(p) == "picture" {
            for s in d.elems(p) {
                if d.tag(s) == "source" {
                    if let Some(v) = d.attr(s, "srcset") {
                        srcsets.push(v);
                    }
                }
            }
        }
        for set in srcsets {
            if let Some(best) = largest(set) {
                if !best.starts_with("data:") {
                    cands.insert(0, best);
                }
            }
        }
        let first = cands.into_iter().next()?;
        Some(if self.base.is_empty() { first } else { css::resolve(&self.base, &first) })
    }

    /// An image for the clone: an https URL, or a local file copied into `assets/`.
    fn asset(&mut self, url: &str) -> Option<String> {
        if url.starts_with("data:") {
            self.count("inline data: images");
            return None;
        }
        let lower = url.to_ascii_lowercase();
        if lower.starts_with("https://") || lower.starts_with("http://") {
            return Some(url.to_string());
        }
        if css::has_scheme(url) {
            return None;
        }
        // a local page: the file next to it, copied into assets/ by name
        let dir = self.local.clone()?;
        let rel = url.split(['?', '#']).next().unwrap_or("");
        let path = dir.join(percent_decode(rel.trim_start_matches('/')));
        if !path.is_file() {
            self.count("images missing next to the local page");
            return None;
        }
        if let Some((_, name)) = self.assets.iter().find(|(p, _)| *p == path) {
            return Some(name.clone());
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).map(slug).filter(|s| !s.is_empty()).unwrap_or_else(|| "image".into());
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("png").to_ascii_lowercase();
        let mut name = format!("{stem}.{ext}");
        let mut k = 2;
        while self.assets.iter().any(|(_, n)| *n == name) {
            name = format!("{stem}-{k}.{ext}");
            k += 1;
        }
        self.assets.push((path, name.clone()));
        Some(name)
    }

    fn push_img(&mut self, img: usize, out: &mut Vec<Item>) {
        let d = self.dom;
        if !self.visible(img) {
            return;
        }
        if self.is_icon(img) {
            self.count("icons");
            return;
        }
        let Some(url) = self.img_url(img) else {
            if d.attr(img, "src").is_some_and(|s| s.starts_with("data:")) {
                self.count("inline data: images");
            }
            return;
        };
        let real = if d.tag(img) == "picture" { d.find(img, "img").unwrap_or(img) } else { img };
        let alt = collapse(d.attr(real, "alt").unwrap_or(""));
        if let Some(src) = self.asset(&url) {
            if self.used_images.insert(src.clone()) || !matches!(out.last(), Some(Item::Img { src: s, .. }) if *s == src) {
                let mo = self.img_motion(img);
                let width = self.small_width(real);
                let px = d.attr(real, "data-nyra-w").and_then(|w| w.parse::<u32>().ok()).unwrap_or(0);
                out.push(Item::Img { src, alt, caption: String::new(), mo, width, px });
            }
        }
    }

    fn iframe(&mut self, id: usize, out: &mut Vec<Item>) {
        let d = self.dom;
        let src = d.attr(id, "src").or_else(|| d.attr(id, "data-src")).map(|s| css::resolve(&self.base, s)).unwrap_or_default();
        let l = src.to_ascii_lowercase();
        if l.contains("google.") && l.contains("/maps") || l.contains("maps.google.") {
            let q = query_param(&src, "q").or_else(|| query_param(&src, "query")).filter(|q| !q.trim().is_empty());
            match q {
                Some(addr) => out.push(Item::Map(collapse(&addr))),
                None => out.push(Item::Embed { url: src, title: d.attr(id, "title").unwrap_or("Map").to_string() }),
            }
            return;
        }
        if (l.starts_with("https://") || l.starts_with("http://")) && (!yt_id(&src).is_empty() || vimeo_id(&src).is_some()) {
            let title = d.attr(id, "title").map(collapse).unwrap_or_default();
            out.push(Item::Video { src, poster: String::new(), title });
            self.videos += 1;
            return;
        }
        if (l.starts_with("https://") || l.starts_with("http://"))
            && (l.contains("youtube")
                || l.contains("youtu.be")
                || l.contains("vimeo")
                || l.contains("spotify")
                || l.contains("soundcloud")
                || l.contains("mapy.cz")
                || l.contains("openstreetmap"))
        {
            out.push(Item::Embed { url: src, title: d.attr(id, "title").map(collapse).unwrap_or_else(|| "Video".into()) });
            return;
        }
        self.count("iframes");
    }

    // ---- lists, tables ----

    fn list(&mut self, id: usize, ordered: bool, out: &mut Vec<Item>) {
        let d = self.dom;
        let lis: Vec<usize> =
            d.elems(id).into_iter().filter(|&k| self.visible(k) && (self.count_words(k) > 0 || self.has_content(k))).collect();
        if lis.is_empty() {
            return;
        }
        let rich = lis.iter().any(|&li| {
            d.descendants(li).into_iter().any(|k| {
                self.visible(k) && (heading_level(d.tag(k)).is_some() || matches!(d.tag(k), "img" | "p" | "ul" | "ol" | "table"))
            })
        });
        if rich {
            if let Some(h) = self.hours_of_rows(&lis) {
                out.push(Item::Hours(h));
                return;
            }
            if ordered && lis.len() >= 2 {
                if let Some(steps) = self.steps_of(&lis) {
                    out.push(Item::Steps(steps));
                    return;
                }
            }
            if let Some(item) = self.group(id) {
                let item = self.moved(id, item);
                out.push(item);
                for k in std::mem::take(&mut self.leftover) {
                    self.flow_node(k, out);
                }
                return;
            }
            for li in lis {
                self.flow_children(li, out);
            }
            return;
        }
        if let Some(h) = self.hours_of_rows(&lis) {
            out.push(Item::Hours(h));
            return;
        }
        // a row of links styled as buttons
        let anchors: Vec<usize> = lis.iter().filter_map(|&li| d.find(li, "a")).collect();
        if anchors.len() == lis.len() && anchors.iter().all(|&a| self.looks_like_button(a)) && lis.len() <= 4 {
            let b: Vec<(String, String)> = anchors.iter().filter_map(|&a| self.button_of(a)).collect();
            if b.len() == lis.len() {
                out.push(Item::Buttons(b));
                return;
            }
        }
        let mut items = Vec::new();
        for li in lis {
            let md = collapse(&self.inline_md(li));
            if !md.is_empty() {
                items.push(md);
            }
        }
        if !items.is_empty() {
            out.push(Item::List { items, ordered });
        }
    }

    fn table(&mut self, id: usize, out: &mut Vec<Item>) {
        let d = self.dom;
        // a table that lays out the page (tables inside it): its cells are boxes, read in order
        if d.find(id, "table").is_some() {
            let cells: Vec<usize> = d
                .descendants(id)
                .into_iter()
                .filter(|&c| matches!(d.tag(c), "td" | "th") && self.visible(c) && self.nearest_table(c) == Some(id))
                .collect();
            for c in cells {
                self.flow_children(c, out);
            }
            return;
        }
        let rows: Vec<usize> = d.find_all(id, "tr").into_iter().filter(|&r| self.visible(r)).collect();
        if rows.is_empty() {
            return;
        }
        // a list laid out as a table (no header, many empty cells): each row is a line
        let cells: Vec<usize> = rows.iter().flat_map(|&r| d.elems(r)).filter(|&c| matches!(d.tag(c), "td" | "th")).collect();
        let empty = cells.iter().filter(|&&c| self.count_words(c) == 0).count();
        if d.find(id, "th").is_none() && cells.len() >= 6 && empty * 10 >= cells.len() * 3 {
            let mut items = Vec::new();
            for &r in &rows {
                let line = collapse(&self.inline_md(r));
                if !line.is_empty() {
                    items.push(line);
                }
            }
            if !items.is_empty() {
                out.push(Item::List { items, ordered: false });
            }
            return;
        }
        let mut head = Vec::new();
        let mut body: Vec<Vec<String>> = Vec::new();
        let mut cells_md = Vec::new();
        for (k, &r) in rows.iter().enumerate() {
            let cells: Vec<usize> = d.elems(r).into_iter().filter(|&c| matches!(d.tag(c), "td" | "th")).collect();
            let texts: Vec<String> = cells.iter().map(|&c| collapse(&self.inline_md(c))).collect();
            let in_head =
                d.tag(d.nodes[r].parent) == "thead" || (k == 0 && cells.iter().all(|&c| d.tag(c) == "th") && cells.len() > 1);
            cells_md.push((texts.clone(), cells.len()));
            if in_head && head.is_empty() {
                head = texts;
            } else if texts.iter().any(|t| !t.is_empty()) {
                body.push(texts);
            }
        }
        // opening hours
        let pairs: Vec<(String, String)> = body.iter().filter(|r| r.len() >= 2).map(|r| (r[0].clone(), r[1..].join(" "))).collect();
        if pairs.len() == body.len() && pairs.len() >= 2 {
            if let Some(h) = hours_of(&pairs) {
                out.push(Item::Hours(h));
                return;
            }
        }
        // the same number of cells in each row
        let width = head.len().max(body.iter().map(|r| r.len()).max().unwrap_or(0));
        for r in &mut body {
            r.resize(width, String::new());
        }
        if !head.is_empty() {
            head.resize(width, String::new());
        }
        if body.is_empty() && head.is_empty() {
            return;
        }
        out.push(Item::Table { head, rows: body });
    }

    fn nearest_table(&self, n: usize) -> Option<usize> {
        let d = self.dom;
        let mut p = d.nodes[n].parent;
        while p != 0 {
            if d.tag(p) == "table" {
                return Some(p);
            }
            p = d.nodes[p].parent;
        }
        None
    }

    fn dl(&mut self, id: usize, out: &mut Vec<Item>) {
        let d = self.dom;
        let mut pairs: Vec<(String, String)> = Vec::new();
        for k in d.descendants(id) {
            if !self.visible(k) {
                continue;
            }
            match d.tag(k) {
                "dt" => pairs.push((collapse(&self.inline_md(k)), String::new())),
                "dd" => {
                    let t = collapse(&self.inline_md(k));
                    match pairs.last_mut() {
                        Some(p) if p.1.is_empty() => p.1 = t,
                        Some(p) => {
                            p.1.push(' ');
                            p.1.push_str(&t);
                        }
                        None => pairs.push((String::new(), t)),
                    }
                }
                _ => {}
            }
        }
        if pairs.is_empty() {
            return;
        }
        let plain_pairs: Vec<(String, String)> = pairs.iter().map(|p| (p.0.clone(), md_plain(&p.1))).collect();
        if let Some(h) = hours_of(&plain_pairs) {
            out.push(Item::Hours(h));
            return;
        }
        let items = pairs
            .into_iter()
            .map(|(t, v)| {
                if t.is_empty() {
                    v
                } else if v.is_empty() {
                    t
                } else {
                    format!("**{}** {v}", t.trim())
                }
            })
            .collect();
        out.push(Item::List { items, ordered: false });
    }

    fn hours_of_rows(&mut self, lis: &[usize]) -> Option<Vec<(String, String)>> {
        let rows: Vec<(String, String)> = lis
            .iter()
            .map(|&li| {
                let t = collapse(&self.text_of(li));
                split_day_label(&t)
            })
            .collect();
        let h = hours_of(&rows)?;
        Some(h)
    }

    // ---- repeated groups ----

    /// A box whose children repeat one shape: cards, a gallery, stats, quotes or steps.
    fn group(&mut self, id: usize) -> Option<Item> {
        let d = self.dom;
        let kids: Vec<usize> =
            d.elems(id).into_iter().filter(|&k| self.visible(k) && self.has_content(k) && !self.is_banner(k)).collect();
        if kids.len() < 2 {
            return None;
        }
        // all photos: a gallery
        let photos: Vec<usize> = kids
            .iter()
            .copied()
            .filter(|&k| {
                let imgs = if matches!(d.tag(k), "img" | "picture") {
                    1
                } else {
                    d.descendants(k).into_iter().filter(|&x| d.tag(x) == "img" && self.visible(x) && !self.is_icon(x)).count()
                };
                imgs == 1 && self.count_words(k) <= 8
            })
            .collect();
        if photos.len() == kids.len() && kids.len() >= 3 && kids.iter().all(|&k| self.count_words(k) <= 3) {
            let mut list = Vec::new();
            for k in kids {
                let img = if matches!(d.tag(k), "img" | "picture") { k } else { d.find(k, "img").unwrap() };
                let alt = collapse(d.attr(img, "alt").unwrap_or("")).to_string();
                let alt = if alt.is_empty() { collapse(&self.text_of(k)) } else { alt };
                if let Some(src) = self.img_url(img).and_then(|u| self.asset(&u)) {
                    list.push((src, alt));
                }
            }
            return (!list.is_empty()).then_some(Item::Gallery(list));
        }
        if let Some(item) = self.media_of(&kids) {
            return Some(item);
        }
        // the same shape
        let shapes: Vec<String> = kids.iter().map(|&k| self.shape(k)).collect();
        let mut best = (0, String::new());
        for s in &shapes {
            let n = shapes.iter().filter(|x| *x == s).count();
            if n > best.0 {
                best = (n, s.clone());
            }
        }
        if best.0 * 4 < kids.len() * 3 {
            return None;
        }
        let names: Vec<String> = std::iter::once(id)
            .chain(kids.iter().copied())
            .flat_map(|k| d.classes(k).into_iter().map(|c| c.to_ascii_lowercase()))
            .collect();
        let named = |ws: &[&str]| names.iter().any(|c| ws.iter().any(|w| c.contains(w)));
        // stats: a number and a short label
        if kids.iter().all(|&k| self.count_words(k) <= 9) {
            let stats: Vec<(String, String)> = kids.iter().filter_map(|&k| self.stat_of(k)).collect();
            if stats.len() == kids.len() {
                return Some(Item::Stats(stats));
            }
        }
        let with_heading = kids.iter().filter(|&&k| self.title_node(k).is_some()).count();
        let with_img = kids
            .iter()
            .filter(|&&k| d.descendants(k).into_iter().any(|x| d.tag(x) == "img" && self.visible(x) && !self.is_icon(x)))
            .count();
        // quotes
        let quoted = kids.iter().filter(|&&k| d.find(k, "blockquote").is_some()).count();
        if (quoted == kids.len() || named(&["testimonial", "review", "quote", "comment"])) && kids.iter().all(|&k| self.count_words(k) >= 6) {
            let mut qs: Vec<Quote> = kids.iter().map(|&k| self.quote_card(k)).collect();
            qs.iter_mut().for_each(split_name);
            return Some(Item::Quotes(qs));
        }
        // steps
        if (d.tag(id) == "ol" || named(&["step", "timeline", "process"])) && with_heading == kids.len() {
            if let Some(s) = self.steps_of(&kids) {
                return Some(Item::Steps(s));
            }
        }
        // cards
        let cardish = kids.iter().all(|&k| {
            self.title_node(k).is_some() || (d.descendants(k).into_iter().any(|x| d.tag(x) == "img") && self.count_words(k) > 0)
        });
        if cardish && (with_heading >= 2 || with_img >= 2) && kids.iter().all(|&k| !self.section_like_big(k)) {
            let mut cards = Vec::new();
            for &k in &kids {
                let mut items = Vec::new();
                if d.tag(k) == "a" || !self.blocky[k] {
                    self.flow_node_as_box(k, &mut items);
                } else {
                    self.flow_children(k, &mut items);
                }
                cards.push(self.card_from(k, items));
            }
            let n = cards.len();
            let cols = if n <= 4 {
                n
            } else if n % 3 == 0 {
                3
            } else if n % 4 == 0 {
                4
            } else {
                3
            };
            return Some(Item::Cards { cols, cards });
        }
        None
    }

    /// A card's content when the card itself is an inline box (`<a class="card">`).
    fn flow_node_as_box(&mut self, k: usize, out: &mut Vec<Item>) {
        if self.blocky[k] {
            self.flow_children(k, out);
        } else {
            let md = collapse(&self.inline_md(k));
            if !md.is_empty() {
                out.push(Item::P(md));
            }
        }
    }

    /// A child too big to be a card: it holds several headings and groups of its own.
    fn section_like_big(&self, k: usize) -> bool {
        let d = self.dom;
        let heads =
            d.descendants(k).into_iter().filter(|&x| self.visible(x) && heading_level(d.tag(x)).is_some_and(|l| l <= 3)).count();
        heads >= 3 || self.count_words(k) > 250
    }

    fn shape(&self, k: usize) -> String {
        let d = self.dom;
        let has = |tags: &[&str]| d.descendants(k).into_iter().any(|x| self.visible(x) && tags.contains(&d.tag(x)));
        format!("{}:{}:{}", d.tag(k), self.title_node(k).is_some() as u8, (has(&["img", "picture"]) || d.tag(k) == "img") as u8)
    }

    /// The element that is the title of a card: a heading, or a short bold line.
    fn title_node(&self, k: usize) -> Option<usize> {
        let d = self.dom;
        let all = d.descendants(k);
        if let Some(h) = all.iter().copied().find(|&x| self.visible(x) && heading_level(d.tag(x)).is_some() && self.count_words(x) > 0)
        {
            return Some(h);
        }
        all.iter().copied().find(|&x| {
            self.visible(x)
                && (matches!(d.tag(x), "strong" | "b") || class_has(&d.classes(x), &["title", "name", "heading"]))
                && (1..=12).contains(&self.count_words(x))
        })
    }

    fn card_from(&mut self, k: usize, items: Vec<Item>) -> Card {
        let d = self.dom;
        let mut c = Card::default();
        if d.tag(k) == "a" {
            c.href = d.attr(k, "href").and_then(|h| self.href(h)).unwrap_or_default();
        }
        let title_md = self.title_node(k).map(|t| collapse(&self.inline_md(t))).unwrap_or_default();
        let mut texts: Vec<String> = Vec::new();
        let mut seen_title = false;
        for it in items {
            match it {
                Item::H(_, t, _) if !seen_title => {
                    seen_title = true;
                    c.title = t;
                }
                Item::P(t) if !seen_title && !title_md.is_empty() && md_plain(&t).trim() == md_plain(&title_md).trim() => {
                    seen_title = true;
                    c.title = t;
                }
                Item::P(t) if !seen_title && words(&t) <= 4 && c.tag.is_empty() && texts.is_empty() => c.tag = t,
                Item::P(t) => texts.push(t),
                Item::Img { src, alt, .. } if c.image.is_empty() => {
                    c.image = src;
                    c.alt = alt;
                }
                Item::List { items, .. } if c.items.is_empty() => c.items = items,
                Item::Buttons(b) => c.buttons.extend(b),
                other => c.extra.push(other),
            }
        }
        if !seen_title && !c.tag.is_empty() {
            c.title = std::mem::take(&mut c.tag);
        }
        // a title that is the card's only link: the whole card links there
        if c.href.is_empty() {
            if let Some(link) = link_only(&c.title) {
                c.title = link.0;
                c.href = link.1;
            } else if c.buttons.len() == 1
                && words(&c.buttons[0].0) <= 3
                && texts.len() <= 2
                && c.extra.is_empty()
                && is_more_link(&c.buttons[0].0)
            {
                // "Read more" -> the card is the link, the label stays as text
                let (label, href) = c.buttons.remove(0);
                texts.push(label);
                c.href = href;
            }
        }
        c.text = texts.join(" ");
        if c.image.is_empty() {
            // a background photo on the card's box
            if let Some(u) = self.st.bg_image(d, k) {
                if let Some(src) = self.asset(&u) {
                    c.image = src;
                }
            }
        }
        c
    }

    fn stat_of(&mut self, k: usize) -> Option<(String, String)> {
        let full = collapse(&self.text_of(k));
        if full.is_empty() {
            return None;
        }
        // the leaf whose text is a number
        let d = self.dom;
        let leaves: Vec<usize> = std::iter::once(k)
            .chain(d.descendants(k))
            .filter(|&x| self.visible(x) && !self.blocky[x] && self.count_words(x) > 0)
            .collect();
        for leaf in leaves {
            let t = collapse(&self.text_of(leaf));
            if is_number(&t) {
                let label = collapse(&full.replacen(&t, "", 1));
                if label.is_empty() || words(&label) > 8 {
                    return None;
                }
                return Some((md_escape(&t), md_escape(&label)));
            }
        }
        // "500+ customers" in one line
        let first = full.split_whitespace().next()?;
        if is_number(first) {
            let rest = full[first.len()..].trim();
            if !rest.is_empty() && words(rest) <= 8 {
                return Some((md_escape(first), md_escape(rest)));
            }
        }
        None
    }

    fn steps_of(&mut self, kids: &[usize]) -> Option<Vec<(String, String)>> {
        let mut out = Vec::new();
        for &k in kids {
            let mut items = Vec::new();
            self.flow_children(k, &mut items);
            let mut label = String::new();
            let mut text = Vec::new();
            for it in items {
                match it {
                    Item::H(_, t, _) if label.is_empty() => label = t,
                    Item::P(t) if label.is_empty() && words(&t) <= 3 => label = t,
                    Item::P(t) => text.push(t),
                    Item::List { items, .. } => text.extend(items),
                    _ => return None,
                }
            }
            if label.is_empty() && text.is_empty() {
                continue;
            }
            out.push((label, text.join(" ")));
        }
        (out.len() >= 2).then_some(out)
    }

    fn quote_of(&mut self, bq: usize) -> Quote {
        let d = self.dom;
        let mut q = Quote::default();
        let mut parts = Vec::new();
        for k in d.elems(bq) {
            if matches!(d.tag(k), "footer" | "cite") {
                q.name = collapse(&self.inline_md(k)).trim_start_matches(['—', '–', '-', ' ']).to_string();
            } else if self.visible(k) {
                parts.push(collapse(&self.inline_md(k)));
            }
        }
        let direct: String = d.nodes[bq].kids.iter().filter(|&&k| d.tag(k) == "#text").map(|&k| md_escape(&d.nodes[k].text)).collect();
        if !collapse(&direct).is_empty() {
            parts.insert(0, collapse(&direct));
        }
        q.text = parts.into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ");
        q
    }

    /// A testimonial card: the longest text is the quote, short lines are the name and place.
    fn quote_card(&mut self, k: usize) -> Quote {
        let d = self.dom;
        if let Some(bq) = d.find(k, "blockquote") {
            let mut q = self.quote_of(bq);
            // the name and place outside the blockquote
            let mut items = Vec::new();
            for c in d.elems(k) {
                if !d.within(bq, c) {
                    self.flow_node(c, &mut items);
                }
            }
            for it in items {
                if let Item::P(t) | Item::H(_, t, _) = it {
                    if q.name.is_empty() {
                        q.name = t.trim_start_matches(['—', '–', '-', ' ']).to_string();
                    } else if q.place.is_empty() {
                        q.place = t;
                    } else {
                        q.place = format!("{} · {t}", q.place);
                    }
                }
            }
            return q;
        }
        let mut items = Vec::new();
        self.flow_children(k, &mut items);
        let mut texts: Vec<String> =
            items.into_iter().filter_map(|i| if let Item::P(t) | Item::H(_, t, _) = i { Some(t) } else { None }).collect();
        let longest = texts.iter().enumerate().max_by_key(|(_, t)| words(t)).map(|(i, _)| i).unwrap_or(0);
        let mut q = Quote::default();
        if texts.is_empty() {
            return q;
        }
        q.text = texts.remove(longest);
        let mut rest = texts.into_iter();
        q.name = rest.next().unwrap_or_default().trim_start_matches(['—', '–', '-', ' ']).to_string();
        q.place = rest.collect::<Vec<_>>().join(" · ");
        q
    }

    // ---- forms ----

    fn form(&mut self, id: usize) -> Option<Form> {
        let d = self.dom;
        let mut f = Form::default();
        if let Some(action) = d.attr(id, "action") {
            let a = action.trim();
            if let Some(m) = a.strip_prefix("mailto:") {
                f.mailto = m.split('?').next().unwrap_or("").to_string();
            } else if !a.is_empty() && !a.starts_with('#') && !a.to_ascii_lowercase().starts_with("javascript:") {
                let abs = css::resolve(&self.base, a);
                if abs.starts_with("https://") || abs.starts_with("http://") {
                    f.to = abs;
                }
            }
        }
        if let Some(h) = d.descendants(id).into_iter().find(|&k| self.visible(k) && heading_level(d.tag(k)).is_some()) {
            f.title = collapse(&self.inline_md(h));
        }
        let mut names: HashSet<String> = HashSet::new();
        let mut radios: Vec<(String, Field)> = Vec::new();
        for k in d.descendants(id) {
            if !self.visible(k) && d.attr(k, "type") != Some("radio") {
                continue;
            }
            let tag = d.tag(k);
            if !matches!(tag, "input" | "select" | "textarea" | "button") {
                continue;
            }
            let ty = d.attr(k, "type").unwrap_or(if tag == "button" { "submit" } else { "text" }).to_ascii_lowercase();
            if tag == "button" || ty == "submit" || ty == "image" {
                if f.send.is_empty() && (ty == "submit" || tag == "button") {
                    let label =
                        if tag == "button" { collapse(&self.text_of(k)) } else { d.attr(k, "value").unwrap_or("").to_string() };
                    f.send = md_escape(&label);
                }
                continue;
            }
            if matches!(ty.as_str(), "hidden" | "reset" | "button" | "file") {
                if ty == "file" {
                    self.count("file upload fields");
                }
                continue;
            }
            let raw_name = d.attr(k, "name").or_else(|| d.attr(k, "id")).unwrap_or("field").to_string();
            let mut name = slug(&raw_name).replace('-', "_");
            if name.is_empty() {
                name = "field".into();
            }
            let label = self.label_of(id, k).unwrap_or_default();
            let kind = match tag {
                "textarea" => "area".to_string(),
                "select" => "select".to_string(),
                _ => match ty.as_str() {
                    "email" | "tel" | "number" | "date" | "url" => ty.clone(),
                    "checkbox" => "check".into(),
                    "radio" => "radio".into(),
                    _ => "text".into(),
                },
            };
            let hint = d.attr(k, "placeholder").map(collapse).unwrap_or_default();
            let mut field = Field {
                name: name.clone(),
                label: md_escape(&if label.is_empty() {
                    if hint.is_empty() {
                        raw_name.clone()
                    } else {
                        hint.clone()
                    }
                } else {
                    label
                }),
                kind,
                req: d.attr(k, "required").is_some() || d.attr(k, "aria-required") == Some("true"),
                hint,
                options: Vec::new(),
            };
            if tag == "select" {
                for o in d.find_all(k, "option") {
                    let t = collapse(&self.text_of(o));
                    if !t.is_empty() && d.attr(o, "value") != Some("") {
                        field.options.push(t);
                    } else if !t.is_empty() && field.hint.is_empty() {
                        field.hint = t;
                    }
                }
            }
            if field.kind == "radio" {
                // radio buttons of one name become one choice
                let opt = md_plain(&field.label);
                if let Some((_, r)) = radios.iter_mut().find(|(n, _)| *n == raw_name) {
                    r.options.push(opt);
                } else {
                    let group_label = self.legend_of(k).unwrap_or_else(|| raw_name.clone());
                    radios.push((
                        raw_name.clone(),
                        Field { kind: "select".into(), label: md_escape(&group_label), options: vec![opt], ..field },
                    ));
                }
                continue;
            }
            if !names.insert(name.clone()) {
                let mut n = 2;
                while names.contains(&format!("{name}_{n}")) {
                    n += 1;
                }
                field.name = format!("{name}_{n}");
                names.insert(field.name.clone());
            }
            f.fields.push(field);
        }
        for (_, r) in radios {
            f.fields.push(r);
        }
        if f.fields.is_empty() {
            // a search box or a button: nothing to send
            self.count("forms without fields");
            return None;
        }
        // the form's other text (an intro line): kept before the fields as the title's line
        Some(f)
    }

    fn label_of(&self, form: usize, k: usize) -> Option<String> {
        let d = self.dom;
        if let Some(i) = d.attr(k, "id") {
            if let Some(l) = d.find_all(form, "label").into_iter().find(|&l| d.attr(l, "for") == Some(i)) {
                let t = collapse(&self.text_of(l));
                if !t.is_empty() {
                    return Some(t.trim_end_matches(['*', ' ', ':']).to_string());
                }
            }
        }
        let mut p = d.nodes[k].parent;
        while p != form && p != 0 {
            if d.tag(p) == "label" {
                let t = collapse(&self.text_of(p));
                if !t.is_empty() {
                    return Some(t.trim_end_matches(['*', ' ', ':']).to_string());
                }
            }
            p = d.nodes[p].parent;
        }
        d.attr(k, "aria-label").map(collapse)
    }

    fn legend_of(&self, k: usize) -> Option<String> {
        let d = self.dom;
        let mut p = d.nodes[k].parent;
        while p != 0 {
            if d.tag(p) == "fieldset" {
                return d.elems(p).into_iter().find(|&x| d.tag(x) == "legend").map(|l| collapse(&self.text_of(l)));
            }
            p = d.nodes[p].parent;
        }
        None
    }

    // ---- navbar, footer ----

    fn read_nav(&mut self, n: usize, site_name: &str) -> Nav {
        let d = self.dom;
        let mut nav = Nav::default();
        let links: Vec<usize> = d.find_all(n, "a").into_iter().filter(|&a| !self.hidden[a] || self.in_menu(a, n)).collect();
        // the brand: a link home, or an element named logo/brand
        // a class named for the brand (`logo`, `navbar-brand`, `site-title`), not a utility class
        // that only colors something in the brand color (`bg-brand`, `text-brand`)
        let brandish = |k: usize| {
            d.classes(k).iter().any(|c| {
                let c = c.to_ascii_lowercase();
                let utility = [
                    "bg-",
                    "text-",
                    "border-",
                    "from-",
                    "to-",
                    "via-",
                    "ring-",
                    "fill-",
                    "stroke-",
                    "shadow-",
                    "hover:",
                    "decoration-",
                ]
                .iter()
                .any(|p| c.starts_with(p) || c.contains(&format!(":{p}")));
                !utility
                    && (c.split(['-', '_']).any(|s| s == "logo" || s == "brand" || s == "logotype")
                        || c.contains("site-title")
                        || c.contains("site-name"))
            })
        };
        let brand_el = d
            .descendants(n)
            .into_iter()
            .find(|&k| brandish(k) && (self.count_words(k) > 0 || d.find(k, "img").is_some() || d.tag(k) == "img"))
            .or_else(|| {
                links.iter().copied().find(|&a| {
                    let h = d.attr(a, "href").unwrap_or("");
                    h == "/"
                        || h == "#"
                        || h == "#top"
                        || h == "./"
                        || h == "index.html"
                        || (!self.base.is_empty() && css::resolve(&self.base, h).trim_end_matches('/') == origin(&self.base))
                })
            });
        let mut brand_link = None;
        if let Some(b) = brand_el {
            let link = if d.tag(b) == "a" { Some(b) } else { links.iter().copied().find(|&a| d.within(b, a) || d.within(a, b)) };
            brand_link = link;
            let text = collapse(&self.text_of(b));
            let img = if d.tag(b) == "img" { Some(b) } else { d.find(b, "img") };
            if let Some(i) = img {
                if let Some(src) = self.img_url(i).and_then(|u| self.asset(&u)) {
                    nav.logo = src;
                }
                if text.is_empty() {
                    nav.brand = md_escape(&collapse(d.attr(i, "alt").unwrap_or("")));
                }
            }
            if !text.is_empty() {
                nav.brand = md_escape(&text);
            }
        }
        if nav.brand.is_empty() {
            nav.brand = md_escape(site_name);
        }
        let mut seen: HashSet<String> = HashSet::new();
        let mut list: Vec<(String, String, usize)> = Vec::new();
        for a in links {
            if Some(a) == brand_link || brand_el.is_some_and(|b| d.within(a, b)) {
                continue;
            }
            let label = collapse(&self.text_of(a));
            let label = if label.is_empty() {
                d.attr(a, "aria-label").or_else(|| d.attr(a, "title")).map(collapse).unwrap_or_default()
            } else {
                label
            };
            if label.is_empty() || words(&label) > 6 {
                if !label.is_empty() {
                    self.count("navbar links with long text");
                }
                continue;
            }
            let Some(href) = d.attr(a, "href").and_then(|h| self.href(h)) else {
                self.count("navbar links to parts not cloned");
                continue;
            };
            if !seen.insert(label.to_lowercase()) {
                continue;
            }
            list.push((md_escape(&label), href, a));
        }
        // the call to action: the last link that looks like a button
        if let Some(pos) = list.iter().rposition(|(_, _, a)| self.looks_like_button(*a)) {
            let (l, h, _) = list.remove(pos);
            nav.cta = Some((l, h));
        }
        for (l, h, _) in list {
            if nav.links.len() >= 8 {
                self.count("navbar links over 8");
                continue;
            }
            nav.links.push((l, h));
        }
        nav
    }

    /// A link in a menu that is closed until clicked (hidden, but part of the navbar).
    fn in_menu(&self, a: usize, nav: usize) -> bool {
        let d = self.dom;
        let mut p = d.nodes[a].parent;
        while p != nav && p != 0 {
            if d.attr(p, "aria-hidden") == Some("true") || d.tag(p) == "template" || d.tag(p) == "script" {
                return false;
            }
            p = d.nodes[p].parent;
        }
        d.attr(a, "aria-hidden") != Some("true")
    }

    fn read_footer(&mut self, f: usize) -> Foot {
        let d = self.dom;
        let mut foot = Foot::default();
        let mut items = Vec::new();
        self.flow_children(f, &mut items);
        // headings name the columns; links go to the links column; the rest are lines
        let mut texts: Vec<String> = Vec::new();
        let mut seen_links: HashSet<String> = HashSet::new();
        for a in d.find_all(f, "a") {
            if !self.visible(a) {
                continue;
            }
            let label = collapse(&self.text_of(a));
            let label = if label.is_empty() {
                d.attr(a, "aria-label").or_else(|| d.attr(a, "title")).map(collapse).unwrap_or_default()
            } else {
                label
            };
            let Some(href) = d.attr(a, "href").and_then(|h| self.href(h)) else { continue };
            let lower = href.to_ascii_lowercase();
            if label.is_empty() {
                continue;
            }
            if lower.starts_with("mailto:") || lower.starts_with("tel:") {
                let line = format!("[{}]({})", md_escape(&label), md_url(&href));
                if !foot.contact.contains(&line) {
                    foot.contact.push(line);
                }
                continue;
            }
            if seen_links.insert(label.to_lowercase()) && words(&label) <= 8 {
                foot.links.push((md_escape(&label), href));
            }
        }
        for it in items {
            self.footer_item(it, &mut foot, &mut texts);
        }
        // the brand and the first text
        if let Some(first) = texts.first() {
            if words(&md_plain(first)) <= 5 && texts.len() > 1 {
                foot.brand = texts.remove(0);
            }
        }
        for t in texts {
            let p = md_plain(&t).to_lowercase();
            if p.contains('©')
                || p.contains("copyright")
                || p.contains("all rights")
                || p.contains("(c)")
                || p.contains("všechna práva")
            {
                foot.small.push(t);
            } else if foot.text.is_empty() && words(&p) >= 4 {
                foot.text = t;
            } else {
                foot.small.push(t);
            }
        }
        if foot.links.len() > 16 {
            let extra = foot.links.len() - 16;
            foot.links.truncate(16);
            *self.dropped.entry("footer links over 16".into()).or_default() += extra;
        }
        foot
    }

    fn footer_item(&mut self, it: Item, foot: &mut Foot, texts: &mut Vec<String>) {
        match it {
            Item::H(_, t, _) => {
                if foot.links_title.is_empty() && !foot.links.is_empty() {
                    foot.links_title = t;
                } else {
                    texts.push(format!("**{}**", md_plain(&t).trim()));
                }
            }
            Item::P(t) => {
                // a line that is only links (already in the links column) adds nothing
                if !only_links(&t) {
                    texts.push(t);
                }
            }
            Item::List { items, .. } => {
                for i in items {
                    if !only_links(&i) {
                        texts.push(i);
                    }
                }
            }
            Item::Address(lines) => {
                for l in lines {
                    if !foot.contact.contains(&l) && !only_links(&l) {
                        foot.contact.push(l);
                    }
                }
            }
            Item::Cards { cards, .. } => {
                for c in cards {
                    if !c.title.is_empty() && foot.links_title.is_empty() && !foot.links.is_empty() {
                        foot.links_title = c.title.clone();
                    } else if !c.title.is_empty() && !only_links(&c.title) {
                        texts.push(format!("**{}**", md_plain(&c.title).trim()));
                    }
                    if !c.text.is_empty() && !only_links(&c.text) {
                        texts.push(c.text);
                    }
                    for i in c.items {
                        if !only_links(&i) {
                            texts.push(i);
                        }
                    }
                    for e in c.extra {
                        self.footer_item(e, foot, texts);
                    }
                }
            }
            Item::Stats(s) => texts.extend(s.into_iter().map(|(v, l)| format!("{v} {l}"))),
            Item::Steps(s) => texts.extend(s.into_iter().map(|(v, l)| format!("{v} {l}"))),
            Item::Quotes(q) => texts.extend(q.into_iter().map(|q| q.text)),
            Item::Table { head, rows } => {
                for r in std::iter::once(head).chain(rows) {
                    let line = r.into_iter().filter(|c| !c.is_empty()).collect::<Vec<_>>().join(" · ");
                    if !line.is_empty() {
                        texts.push(line);
                    }
                }
            }
            Item::Hours(h) => texts.extend(h.into_iter().map(|(l, s)| format!("{l} {s}"))),
            Item::Map(a) => foot.contact.push(md_escape(&a)),
            _ => self.count("footer content (images, forms)"),
        }
    }

    // ---- the theme ----

    fn theme(&self, body: usize, theme_color: Option<&str>) -> Theme {
        let d = self.dom;
        let st = self.st;
        let mut t = Theme::default();
        // the page background: the body, the html, or the first box that fills the page
        let html = d.find(0, "html").unwrap_or(0);
        let mut bg = st.bg(d, body).or_else(|| st.bg(d, html));
        if bg.is_none() {
            let mut n = body;
            for _ in 0..5 {
                let kids: Vec<usize> = d.elems(n).into_iter().filter(|&k| self.visible(k) && self.has_content(k)).collect();
                if kids.len() != 1 {
                    // the main box among a few
                    if let Some(&m) = kids.iter().find(|&&k| d.tag(k) == "main") {
                        bg = st.bg(d, m);
                    }
                    break;
                }
                n = kids[0];
                if let Some(c) = st.bg(d, n) {
                    bg = Some(c);
                    break;
                }
            }
        }
        // what the browser computed (`--browser`) wins over the reader's cascade
        if let Some(c) = d.attr(body, "data-nyra-bg").and_then(css::parse_color).filter(|c| c.a >= 0.5) {
            bg = Some(c);
        }
        let bg_c = bg.unwrap_or(Rgb { r: 255.0, g: 255.0, b: 255.0, a: 1.0 });
        if bg.is_some() {
            t.bg = Some(bg_c.hex());
        }
        t.dark = bg_c.luma() < 0.2;
        // the text color: the body's, or the most common color of paragraphs
        let mut ink = d.attr(body, "data-nyra-ink").and_then(css::parse_color).or_else(|| st.color(d, body));
        if ink.is_none() {
            let mut counts: HashMap<String, (usize, Rgb)> = HashMap::new();
            for p in d.find_all(body, "p").into_iter().filter(|&p| self.visible(p)).take(200) {
                if let Some(c) = st.color(d, p) {
                    counts.entry(c.hex()).or_insert((0, c)).0 += 1;
                }
            }
            ink = counts.into_values().max_by_key(|(n, _)| *n).map(|(_, c)| c);
        }
        if let Some(c) = ink {
            if css::contrast(&c, &bg_c) >= 4.5 {
                t.ink = Some(c.hex());
            }
        }
        // brand colors: buttons' backgrounds count most, then links and headings, then any
        // colored background
        let mut votes: Vec<(Rgb, f64)> = Vec::new();
        let vote = |c: Rgb, w: f64, votes: &mut Vec<(Rgb, f64)>| {
            if !c.colorful() {
                return;
            }
            if let Some(v) = votes.iter_mut().find(|(x, _)| x.distance(&c) < 18.0) {
                v.1 += w;
            } else {
                votes.push((c, w));
            }
        };
        for k in d.descendants(body) {
            if !self.visible(k) || d.tag(k) == "#text" {
                continue;
            }
            let tag = d.tag(k);
            let buttonish =
                tag == "button" || (tag == "a" && self.looks_like_button(k)) || class_has(&d.classes(k), &["btn", "button"]);
            if let Some(c) = st.bg(d, k) {
                vote(c, if buttonish { 6.0 } else { 1.0 }, &mut votes);
            }
            if tag == "a" || heading_level(tag).is_some() {
                if let Some(c) = st.color(d, k) {
                    vote(c, 1.0, &mut votes);
                }
            }
        }
        if let Some(c) = theme_color.and_then(css::parse_color) {
            vote(c, 3.0, &mut votes);
        }
        votes.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        if let Some((p, _)) = votes.first() {
            t.primary = Some(p.hex());
            let (ph, _, pl) = p.hsl();
            if let Some((a, _)) = votes.iter().skip(1).find(|(c, _)| {
                let (h, _, l) = c.hsl();
                let dh = (h - ph).abs().min(360.0 - (h - ph).abs());
                dh > 25.0 || (l - pl).abs() > 0.15
            }) {
                t.accent = Some(a.hex());
            }
            // one brand color: the kit's buttons in the accent color take it too
            if t.accent.is_none() {
                t.accent = t.primary.clone();
            }
        }
        // fonts: the body's, and the headings' when they differ
        t.font = st.font(d, body).or_else(|| d.find_all(body, "p").into_iter().find_map(|p| st.font(d, p)));
        let head_font = ["h1", "h2", "h3"]
            .iter()
            .find_map(|h| d.find_all(body, h).into_iter().find(|&x| self.visible(x)).and_then(|x| st.font(d, x)));
        if head_font.is_some() && head_font != t.font {
            t.display = head_font;
        }
        // the layout of the text: the column around the section headings, and their alignment
        // (the section headings, or on a page without them its paragraphs)
        let visible = |tag: &str| -> Vec<usize> {
            d.find_all(body, tag)
                .into_iter()
                .filter(|&h| self.visible(h) && !self.foot.is_some_and(|f| d.within(h, f)))
                .take(6)
                .collect()
        };
        let heads = ["h2", "h1", "p"].iter().map(|t| visible(t)).find(|v| !v.is_empty()).unwrap_or_default();
        if !heads.is_empty() {
            // (a page without styles keeps the kit's centered headings)
            let styled = st.color(d, body).is_some() || st.font(d, body).is_some() || bg.is_some();
            let centered = heads.iter().filter(|&&h| st.get(d, h, "text-align").is_some_and(|a| a.trim() == "center")).count();
            t.left = styled && centered * 2 < heads.len();
            let narrow = heads.iter().filter(|&&h| {
                let mut n = h;
                while n != 0 {
                    if st.max_width(d, n).is_some_and(|w| (400.0..=960.0).contains(&w)) {
                        return true;
                    }
                    if n == body {
                        break;
                    }
                    n = d.nodes[n].parent;
                }
                false
            });
            t.narrow = narrow.count() * 2 > heads.len();
        }
        // the corner radius of cards (or buttons when there are no cards)
        let mut radii: Vec<f64> = Vec::new();
        for k in d.descendants(body) {
            if !self.visible(k) {
                continue;
            }
            if class_has(&d.classes(k), &["card", "tile", "panel", "box"]) {
                if let Some(r) = st.radius(d, k) {
                    radii.push(r);
                }
            }
        }
        radii.retain(|r| *r < 60.0);
        if !radii.is_empty() {
            radii.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            t.radius = Some(radii[radii.len() / 2].round() as u32);
        }
        t
    }
}

// ---- app layouts, media, motion, server features ------------------------------------------------------

impl Reader<'_> {
    /// An app's side navigation: the column the browser saw down the left side of the window
    /// (`data-nyra="sidebar"`), or a box named for one (`sidebar`, `sidenav`, `docs-nav`, ...)
    /// that is mostly links and sits before the main content.
    fn find_side(&self, body: usize) -> Option<usize> {
        let d = self.dom;
        let all = d.descendants(body);
        if let Some(&s) = all.iter().find(|&&n| d.attr(n, "data-nyra") == Some("sidebar") && self.visible(n)) {
            return Some(s);
        }
        let main = all.iter().copied().find(|&n| matches!(d.tag(n), "main" | "article") && self.visible(n));
        let body_words = self.count_words(body);
        let mut best: Option<(usize, usize)> = None;
        for &n in &all {
            if !self.visible(n) || !matches!(d.tag(n), "aside" | "nav" | "div" | "section") || self.inside(n, &["footer", "header"]) {
                continue;
            }
            let mut names: Vec<String> = d.classes(n).iter().map(|c| c.to_ascii_lowercase()).collect();
            if let Some(i) = d.attr(n, "id") {
                names.push(i.to_ascii_lowercase());
            }
            let named = names.iter().any(|c| {
                ["sidebar", "side-bar", "sidenav", "side-nav", "side_nav", "leftnav", "left-nav", "leftmenu", "left-menu", "docs-nav", "docsnav"]
                    .iter()
                    .any(|w| c.contains(w))
                    || c == "guide"
            });
            if !named || names.iter().any(|c| c.contains("right")) {
                continue;
            }
            // a column after the main content is a blog's widgets, not an app's navigation
            if main.is_some_and(|m| m < n && !d.within(n, m)) && !names.iter().any(|c| c.contains("left")) {
                continue;
            }
            let links: Vec<usize> = d.find_all(n, "a").into_iter().filter(|&a| self.visible(a)).collect();
            if links.len() < 5 {
                continue;
            }
            let link_words: usize = links.iter().map(|&a| self.count_words(a)).sum();
            let words = self.count_words(n);
            if words == 0 || link_words * 10 < words * 4 || words * 2 > body_words {
                continue;
            }
            if best.is_some_and(|(b, _)| d.within(n, b)) {
                continue;
            }
            if best.is_none_or(|(_, l)| links.len() > l) {
                best = Some((n, links.len()));
            }
        }
        best.map(|b| b.0)
    }

    fn read_side(&mut self, n: usize) -> Side {
        let mut side = Side::default();
        let mut links = Vec::new();
        self.side_walk(n, &mut links);
        let mut seen: HashSet<(String, String)> = HashSet::new();
        for l in links {
            if side.links.len() >= 80 {
                self.count("side navigation links over 80");
                continue;
            }
            if seen.insert(l.clone()) {
                side.links.push(l);
            }
        }
        side
    }

    /// The links of a side navigation in order; headings and lines of text between them as
    /// entries without a link.
    fn side_walk(&mut self, n: usize, out: &mut Vec<(String, String)>) {
        let d = self.dom;
        for k in d.elems(n) {
            if !self.visible(k) {
                continue;
            }
            let t = d.tag(k);
            if t == "a" {
                let mut label = collapse(&self.text_of(k));
                if label.is_empty() {
                    label = d.attr(k, "title").or_else(|| d.attr(k, "aria-label")).map(collapse).unwrap_or_default();
                }
                if label.is_empty() || words(&label) > 10 {
                    continue;
                }
                let href = d.attr(k, "href").and_then(|h| self.href(h)).unwrap_or_else(|| "#top".into());
                let icon = if self.has_icon(k) { side_icon(&label) } else { None };
                let text = md_escape(&label);
                out.push((icon.map(|i| format!(":{i}: {text}")).unwrap_or(text), href));
                continue;
            }
            let has_link = d.find(k, "a").is_some();
            if heading_level(t).is_some() || (!has_link && self.count_words(k) > 0 && (!self.blocky[k] || d.find(k, "img").is_none())) {
                let l = collapse(&self.text_of(k));
                if !l.is_empty() && !has_link {
                    out.push((md_escape(&l), String::new()));
                    continue;
                }
            }
            if matches!(t, "input" | "select" | "button" | "form") {
                continue;
            }
            self.side_walk(k, out);
        }
    }

    fn has_icon(&self, a: usize) -> bool {
        let d = self.dom;
        d.descendants(a).into_iter().any(|k| {
            matches!(d.tag(k), "svg" | "yt-icon" | "i") || (d.tag(k) == "img" && self.is_icon(k)) || class_has(&d.classes(k), &["icon"])
        })
    }

    // ---- what needs a server ----

    /// The features of the page that need a server, and the `// TODO(server)` notes for the top
    /// of the page (sign-in, likes, site search: they live in the header).
    fn find_server(&mut self, body: usize) -> Vec<String> {
        let d = self.dom;
        let mut found: Vec<&'static str> = Vec::new();
        let mut add = |f: &'static str| {
            if !found.contains(&f) {
                found.push(f);
            }
        };
        for k in d.descendants(body) {
            if !d.is_elem(k) {
                continue;
            }
            let tag = d.tag(k);
            let ty = d.attr(k, "type").unwrap_or("").to_ascii_lowercase();
            if tag == "input" && ty == "password" {
                add("sign-in and accounts");
            }
            if !self.visible(k) {
                continue;
            }
            if tag == "input" && ty == "file" {
                add("uploads");
            }
            if (tag == "form" && (d.attr(k, "role") == Some("search") || d.attr(k, "action").is_some_and(|a| a.contains("search") || a.contains("result"))))
                || (tag == "input" && self.searchish(k) && self.inside(k, &["header", "nav", "form"]))
            {
                add("search over the whole site");
            }
            if self.server_box(k).is_some() {
                add(self.server_box(k).unwrap());
            }
            if matches!(tag, "a" | "button") || d.attr(k, "role") == Some("button") {
                let label = collapse(&self.text_of(k)).to_lowercase();
                let label = if label.is_empty() { d.attr(k, "aria-label").map(|l| l.to_lowercase()).unwrap_or_default() } else { label };
                let label = label.trim();
                let href = d.attr(k, "href").unwrap_or("").to_ascii_lowercase();
                if [
                    "sign in", "sign up", "log in", "login", "register", "create account", "my account", "account", "přihlásit", "přihlásit se",
                    "registrace", "anmelden", "registrieren", "connexion", "se connecter", "iniciar sesión", "accedi",
                ]
                .contains(&label)
                    || href.contains("/login")
                    || href.contains("/signin")
                    || href.contains("accounts.google.")
                {
                    add("sign-in and accounts");
                }
                if ["subscribe", "like", "dislike", "follow", "odebírat", "sledovat", "abonnieren", "join"].contains(&label) {
                    add("subscriptions and likes");
                }
                if ["add to cart", "add to basket", "add to bag", "buy now", "buy", "checkout", "cart", "basket", "do košíku", "košík", "koupit", "in den warenkorb", "warenkorb", "panier"]
                    .iter()
                    .any(|w| label == *w || label.starts_with(&format!("{w} ")))
                    || href.contains("/cart")
                    || href.contains("/checkout")
                    || href.contains("basket")
                {
                    add("a cart and payments");
                }
            }
        }
        self.server = found.iter().map(|f| f.to_string()).collect();
        let mut top = Vec::new();
        for f in ["sign-in and accounts", "subscriptions and likes", "search over the whole site"] {
            if found.contains(&f) {
                top.push(todo_text(f));
                self.todo_placed.insert(f.to_string());
            }
        }
        top
    }

    /// A box of the page whose content a server makes per visitor (comments, recommendations).
    fn server_box(&self, id: usize) -> Option<&'static str> {
        let d = self.dom;
        let tag = d.tag(id);
        let ident = d.attr(id, "id").unwrap_or("").to_ascii_lowercase();
        let classes = d.classes(id);
        if tag == "ytd-comments" || matches!(ident.as_str(), "comments" | "comment-section" | "disqus_thread" | "respond")
            || classes.iter().any(|c| matches!(c.to_ascii_lowercase().as_str(), "comments" | "comments-area" | "comment-list" | "commentlist"))
        {
            return Some("comments");
        }
        if tag == "ytd-watch-next-secondary-results-renderer" || ident == "related" || ident == "recommendations" {
            return Some("recommendations");
        }
        None
    }

    /// A `// TODO(server)` note in the program, once per feature.
    fn todo(&mut self, feature: &str, out: &mut Vec<Item>) {
        if !self.server.iter().any(|f| f == feature) {
            self.server.push(feature.to_string());
        }
        if self.todo_placed.insert(feature.to_string()) {
            out.push(Item::Todo(todo_text(feature)));
        }
    }

    /// An input that searches (or filters) what follows it.
    fn searchish(&self, id: usize) -> bool {
        let d = self.dom;
        let ty = d.attr(id, "type").unwrap_or("text").to_ascii_lowercase();
        if ty == "search" {
            return true;
        }
        if !matches!(ty.as_str(), "text" | "") {
            return false;
        }
        let hint = format!(
            "{} {} {}",
            d.attr(id, "placeholder").unwrap_or(""),
            d.attr(id, "aria-label").unwrap_or(""),
            d.attr(id, "name").unwrap_or("")
        )
        .to_lowercase();
        ["search", "filter", "find", "hledat", "vyhledat", "suchen", "rechercher", "buscar", "query"].iter().any(|w| hint.contains(w))
            || matches!(d.attr(id, "name"), Some("q") | Some("s") | Some("search_query"))
    }

    // ---- videos ----

    /// A video player box (a `<video>` with the player's own buttons around it).
    fn is_player(&self, id: usize) -> bool {
        let d = self.dom;
        if !matches!(d.tag(id), "div" | "section" | "figure" | "ytd-player") {
            return false;
        }
        let mut names: Vec<String> = d.classes(id).iter().map(|c| c.to_ascii_lowercase()).collect();
        names.push(d.attr(id, "id").unwrap_or("").to_ascii_lowercase());
        let named = names.iter().any(|c| c.contains("video-player") || c.contains("html5-video-player") || c == "movie_player" || c == "player" || c.starts_with("vjs") || c.contains("plyr"));
        named && d.find(id, "video").is_some()
    }

    /// A `<video>` (or a player around one) that the clone can play: its file, or the YouTube or
    /// Vimeo video the page is about (a player that streams with scripts has no file).
    fn video_item(&mut self, id: usize) -> Option<Item> {
        let d = self.dom;
        let v = if d.tag(id) == "video" { id } else { d.find(id, "video")? };
        let poster = d.attr(v, "poster").map(|p| css::resolve(&self.base, p)).and_then(|p| self.asset(&p)).unwrap_or_default();
        let src = d
            .attr(v, "src")
            .map(str::to_string)
            .or_else(|| d.find_all(v, "source").into_iter().find_map(|s| d.attr(s, "src").map(str::to_string)))
            .unwrap_or_default();
        let src = if src.is_empty() || src.starts_with("blob:") || src.starts_with("data:") { String::new() } else { css::resolve(&self.base, &src) };
        let title = d.attr(v, "title").or_else(|| d.attr(v, "aria-label")).map(collapse).unwrap_or_default();
        let page_title = short_title(&self.title.replace(" - YouTube", ""));
        if src.starts_with("https://") || src.starts_with("http://") {
            self.videos += 1;
            return Some(Item::Video { src, poster, title });
        }
        // the page of a YouTube or Vimeo video: its player plays here from the video site
        if !yt_id(&self.base).is_empty() || vimeo_id(&self.base).is_some() {
            self.videos += 1;
            return Some(Item::Video { src: self.base.clone(), poster, title: if title.is_empty() { page_title } else { title } });
        }
        let og = d
            .find_all(0, "meta")
            .into_iter()
            .find(|&m| matches!(d.attr(m, "property"), Some("og:video") | Some("og:video:url") | Some("og:video:secure_url")))
            .and_then(|m| d.attr(m, "content"))
            .map(str::to_string);
        if let Some(og) = og.filter(|o| !yt_id(o).is_empty() || vimeo_id(o).is_some()) {
            self.videos += 1;
            return Some(Item::Video { src: og, poster, title });
        }
        if !self.server.iter().any(|f| f == "video streaming") {
            self.server.push("video streaming".into());
        }
        None
    }

    // ---- tabs, carousels, marquees ----

    /// Tabs whose panels are in the page: the box's children in order, the tab list and its
    /// panels as one `Tabs` item. False when the box has none (or a child holds them all).
    fn tabs(&mut self, id: usize, out: &mut Vec<Item>) -> bool {
        let d = self.dom;
        let desc = d.descendants(id);
        let tabs: Vec<usize> = desc.iter().copied().filter(|&k| d.attr(k, "role") == Some("tab") && self.visible(k)).collect();
        if tabs.len() < 2 || tabs.len() > 12 {
            return false;
        }
        let mut panels = Vec::new();
        for &t in &tabs {
            let target = d
                .attr(t, "aria-controls")
                .map(str::to_string)
                .or_else(|| d.attr(t, "href").and_then(|h| h.strip_prefix('#')).map(str::to_string))
                .or_else(|| d.attr(t, "data-bs-target").or_else(|| d.attr(t, "data-target")).and_then(|h| h.strip_prefix('#')).map(str::to_string));
            let Some(target) = target else { return false };
            let Some(p) = desc.iter().copied().find(|&k| d.attr(k, "id") == Some(target.as_str())) else { return false };
            panels.push(p);
        }
        // a child that holds the tabs and all the panels reads them itself
        let list = d.nodes[tabs[0]].parent;
        let kids = d.elems(id);
        if kids.iter().any(|&c| d.within(list, c) && panels.iter().all(|&p| d.within(p, c))) {
            return false;
        }
        let labels: Vec<String> = tabs.iter().map(|&t| collapse(&self.inline_md(t))).collect();
        if labels.iter().any(|l| l.is_empty()) {
            return false;
        }
        let mut done = false;
        for c in kids {
            if !self.visible(c) {
                continue;
            }
            let holds_list = d.within(list, c);
            let holds_panel = panels.iter().any(|&p| d.within(p, c));
            if holds_list || holds_panel {
                if !done {
                    done = true;
                    let mut items = Vec::new();
                    for &p in &panels {
                        let mut v = Vec::new();
                        self.flow_children(p, &mut v);
                        items.push(v);
                    }
                    out.push(Item::Tabs { labels: labels.clone(), panels: items });
                }
                continue;
            }
            self.flow_node(c, out);
        }
        true
    }

    /// A tab list whose panels are not in the page (each tab opens another page): a row of
    /// links, the current one marked.
    fn tab_links(&mut self, id: usize) -> Option<Item> {
        let d = self.dom;
        let mut out = Vec::new();
        for t in d.descendants(id).into_iter().filter(|&k| d.attr(k, "role") == Some("tab") && self.visible(k)) {
            let label = collapse(&self.text_of(t));
            if label.is_empty() || words(&label) > 5 {
                continue;
            }
            let link = if d.tag(t) == "a" { Some(t) } else { d.find(t, "a") };
            let href = link.and_then(|a| d.attr(a, "href")).and_then(|h| self.href(h)).unwrap_or_else(|| if self.base.is_empty() { "#top".into() } else { self.base.clone() });
            out.push((md_escape(&label), href, d.attr(t, "aria-selected") == Some("true")));
        }
        (out.len() >= 2).then_some(Item::TabLinks(out))
    }

    /// The width of a small picture (an avatar, a logo: 65 to 400 px) as the browser drew it or
    /// its `width` says, for `img(..., width:)`; "" for a picture as wide as its column.
    fn small_width(&self, img: usize) -> String {
        let d = self.dom;
        let w = d
            .attr(img, "data-nyra-w")
            .or_else(|| d.attr(img, "width"))
            .and_then(|v| v.trim().trim_end_matches("px").parse::<f64>().ok())
            .unwrap_or(0.0);
        if (65.0..=400.0).contains(&w) {
            format!("{}px", w.round())
        } else {
            String::new()
        }
    }

    /// A carousel (slider) of pictures: a gallery of all its slides (the copies a carousel makes
    /// for its endless loop left out).
    fn carousel(&mut self, id: usize) -> Option<Item> {
        let d = self.dom;
        let names: Vec<String> = d.classes(id).iter().map(|c| c.to_ascii_lowercase()).collect();
        let named = d.attr(id, "aria-roledescription") == Some("carousel")
            || names.iter().any(|c| {
                ["carousel", "swiper", "slick-slider", "splide", "glide", "owl-carousel", "flickity", "embla", "slideshow", "slider"]
                    .iter()
                    .any(|w| c == w || c.starts_with(&format!("{w}-container")) || (c.contains(w) && !c.contains("item") && !c.contains("slide-") && !c.contains("range")))
            });
        if !named || d.find(id, "h1").is_some() {
            return None;
        }
        let mut photos: Vec<(String, String)> = Vec::new();
        let mut slides = 0;
        for k in d.descendants(id) {
            if d.tag(k) != "img" || self.is_icon(k) {
                continue;
            }
            // a copy of a slide, made for the loop
            let mut p = k;
            let mut copy = false;
            while p != id && p != 0 {
                if d.classes(p).iter().any(|c| c.contains("duplicate") || c.contains("cloned") || *c == "clone") {
                    copy = true;
                    break;
                }
                p = d.nodes[p].parent;
            }
            if copy {
                continue;
            }
            slides += 1;
            if let Some(src) = self.img_url(k).and_then(|u| self.asset(&u)) {
                if !photos.iter().any(|(s, _)| *s == src) {
                    let alt = collapse(d.attr(k, "alt").unwrap_or(""));
                    photos.push((src, alt));
                }
            }
        }
        let words = self.count_words(id);
        if photos.len() < 2 || words > slides * 15 + 10 {
            return None;
        }
        Some(Item::Gallery(photos))
    }

    /// An endless strip (a box whose animation slides it left forever): its words, or its
    /// pictures (logos).
    fn marquee_of(&mut self, id: usize) -> Option<Item> {
        let d = self.dom;
        let mut photos: Vec<(String, String)> = Vec::new();
        for k in d.find_all(id, "img") {
            if let Some(src) = self.img_url(k).and_then(|u| self.asset(&u)) {
                if !photos.iter().any(|(s, _)| *s == src) {
                    photos.push((src, collapse(d.attr(k, "alt").unwrap_or(""))));
                }
            }
        }
        let mut texts: Vec<String> = Vec::new();
        for (_, t) in self.leaf_texts(id) {
            if !texts.contains(&t) {
                texts.push(t);
            }
        }
        if photos.len() >= 3 {
            return Some(Item::Strip(photos));
        }
        (texts.len() >= 2).then(|| Item::Marquee(texts.into_iter().map(|t| md_escape(&t)).collect()))
    }

    // ---- motion ----

    /// The motion of an element or the first of its descendants that has some (at most `limit`
    /// nodes looked at).
    fn motion_within(&self, id: usize) -> Mo {
        let d = self.dom;
        let mut m = self.mo.get(id).clone();
        if m.anim.is_empty() || m.hover.is_empty() {
            for k in d.descendants(id).into_iter().take(80) {
                let x = self.mo.get(k);
                if m.anim.is_empty() && !x.anim.is_empty() {
                    m.anim = x.anim.clone();
                }
                // the hover of the card's box or its picture (a button's own hover is not the card's)
                let boxy = d.tag(k) == "img" || (matches!(d.tag(k), "a" | "div" | "article" | "figure") && d.find(k, "img").is_some());
                if m.hover.is_empty() && !x.hover.is_empty() && boxy {
                    // a picture that grows inside a card: the card zooms
                    m.hover = if x.hover == "zoom" || d.tag(k) == "img" { "zoom".into() } else { x.hover.clone() };
                }
            }
        }
        m
    }

    /// The motion of a repeated group: children that come in one after another stagger, a hover
    /// they share moves each of them, counters count.
    fn moved(&self, id: usize, item: Item) -> Item {
        let d = self.dom;
        let own = self.mo.get(id).clone();
        let kids: Vec<usize> = d.elems(id).into_iter().filter(|&k| self.visible(k)).collect();
        let subs: Vec<Mo> = kids.iter().map(|&k| self.motion_within(k)).collect();
        let mut mo = Mo::default();
        let animated = subs.iter().filter(|m| !m.anim.is_empty()).count();
        if subs.iter().any(|m| m.anim == "count") {
            mo.anim = "count".into();
        } else if animated >= 2 {
            mo.anim = "stagger".into();
        } else if !own.anim.is_empty() {
            mo.anim = own.anim.clone();
        }
        let mut hovers: BTreeMap<String, usize> = BTreeMap::new();
        for m in &subs {
            if !m.hover.is_empty() {
                *hovers.entry(m.hover.clone()).or_default() += 1;
            }
        }
        if let Some((h, n)) = hovers.into_iter().max_by_key(|(_, n)| *n) {
            if n * 2 >= kids.len().max(1) {
                mo.hover = h;
            }
        }
        mo.looping = own.looping.clone();
        mo.scroll = own.scroll.clone();
        // motion words that a component cannot take
        if matches!(item, Item::Gallery(_)) && mo.anim == "count" {
            mo.anim.clear();
        }
        if mo.is_empty() {
            item
        } else {
            Item::Moved { mo, item: Box::new(item) }
        }
    }

    /// How a section moves: its own box, else its heading or the first text that comes in.
    fn group_motion(&self, g: &[usize]) -> Mo {
        let d = self.dom;
        let mut m = Mo::default();
        for &n in g {
            m.or(self.mo.get(n));
        }
        if m.anim.is_empty() {
            for &n in g {
                for k in d.descendants(n).into_iter().take(400) {
                    let x = self.mo.get(k);
                    let word = crate::web::motion::ANIM.contains(&x.anim.as_str()) && !matches!(x.anim.as_str(), "count" | "none");
                    if word && self.drawn(k) && (heading_level(d.tag(k)).is_some() || matches!(d.tag(k), "p" | "div" | "section" | "header")) {
                        m.anim = x.anim.clone();
                        break;
                    }
                }
                if !m.anim.is_empty() {
                    break;
                }
            }
        }
        // a section has no hover or loop of its own in the kit
        m.hover.clear();
        m.looping.clear();
        m
    }

    /// The motion of a picture: its own, or that of the box around it alone.
    fn img_motion(&self, img: usize) -> Mo {
        let d = self.dom;
        let mut m = self.mo.get(img).clone();
        let mut p = d.nodes[img].parent;
        for _ in 0..2 {
            if p == 0 || self.count_words(p) > 0 || d.find_all(p, "img").len() > 1 {
                break;
            }
            m.or(self.mo.get(p));
            p = d.nodes[p].parent;
        }
        if m.anim == "count" {
            m.anim.clear();
        }
        m
    }

    // ---- videos and products ----

    /// The visible texts of a box in reading order: (element, text) of each element that holds
    /// only inline content, and lone text directly in a box.
    fn leaf_texts(&self, k: usize) -> Vec<(usize, String)> {
        let d = self.dom;
        let mut out = Vec::new();
        let mut todo = vec![k];
        while let Some(n) = todo.pop() {
            if !self.drawn(n) || matches!(d.tag(n), "button" | "svg" | "select" | "input" | "textarea") {
                continue;
            }
            if d.tag(n) == "#text" {
                let t = collapse(&d.nodes[n].text);
                if !t.is_empty() && self.blocky[d.nodes[n].parent] {
                    out.push((n, t));
                }
                continue;
            }
            // a line of separate pieces (`<p><span>Channel</span> <span>12K views</span></p>`)
            let pieces: Vec<usize> = d.elems(n).into_iter().filter(|&c| self.drawn(c) && !self.drawn_text(c).trim().is_empty()).collect();
            let loose_text = d.nodes[n].kids.iter().any(|&c| d.tag(c) == "#text" && !d.nodes[c].text.trim().is_empty());
            if n != k && !self.blocky[n] && d.is_elem(n) && pieces.len() >= 2 && !loose_text && d.tag(n) != "a" && heading_level(d.tag(n)).is_none() {
                for &c in pieces.iter().rev() {
                    todo.push(c);
                }
                continue;
            }
            if n != k && !self.blocky[n] && d.is_elem(n) {
                if d.tag(n) == "a" && self.looks_like_button(n) {
                    continue;
                }
                // the text drawn (a label only for screen readers is not)
                let raw = collapse(&self.drawn_text(n));
                if !raw.is_empty() {
                    let label = d.attr(n, "aria-label").unwrap_or("").to_lowercase();
                    // "90K" with the label "90 thousand views"
                    let t = if label.ends_with(" views") && !raw.to_lowercase().contains("view") { format!("{raw} views") } else { raw };
                    out.push((n, t));
                }
                continue;
            }
            for &c in d.nodes[n].kids.iter().rev() {
                todo.push(c);
            }
        }
        out
    }

    /// The text of a subtree that is drawn, without the labels of `aria-label`.
    fn drawn_text(&self, n: usize) -> String {
        let d = self.dom;
        if d.tag(n) == "#text" {
            return d.nodes[n].text.clone();
        }
        if !self.drawn(n) || matches!(d.tag(n), "script" | "style" | "button" | "svg") {
            return String::new();
        }
        d.nodes[n].kids.iter().map(|&k| self.drawn_text(k)).collect::<Vec<_>>().join(" ")
    }

    /// A grid of video thumbnails (a length on the picture, "views", links to videos) or of
    /// products (a price): each child a picture, a title, a link and a meta line.
    fn media_of(&mut self, kids: &[usize]) -> Option<Item> {
        let d = self.dom;
        if kids.len() < 3 {
            return None;
        }
        let mut items: Vec<(usize, MediaItem)> = Vec::new();
        let mut rest = Vec::new();
        let (mut videoish, mut priced) = (0, 0);
        for &k in kids {
            let imgs: Vec<usize> = std::iter::once(k)
                .chain(d.descendants(k))
                .filter(|&x| d.tag(x) == "img" && self.drawn(x) && !self.is_icon(x))
                .collect();
            let wc = self.count_words(k);
            if imgs.len() > 2 || wc > 80 || wc == 0 {
                rest.push(k);
                continue;
            }
            let mut it = MediaItem::default();
            let title_node = self.title_node(k);
            it.title = title_node.map(|t| collapse(&self.text_of(t))).unwrap_or_default();
            let leaves = self.leaf_texts(k);
            let links: Vec<usize> = std::iter::once(k).chain(d.descendants(k)).filter(|&x| d.tag(x) == "a" && d.attr(x, "href").is_some()).collect();
            if it.title.is_empty() {
                it.title = links.iter().map(|&a| collapse(&self.text_of(a))).max_by_key(|t| t.len()).unwrap_or_default();
            }
            if it.title.is_empty() {
                rest.push(k);
                continue;
            }
            let title_link = links.iter().copied().find(|&a| title_node.is_some_and(|t| d.within(t, a) || d.within(a, t)));
            let video_link = links.iter().copied().find(|&a| d.attr(a, "href").and_then(|h| self.href(h)).is_some_and(|h| !yt_id(&h).is_empty() || vimeo_id(&h).is_some()));
            let picture_link = imgs.first().and_then(|&i| links.iter().copied().find(|&a| d.within(i, a)));
            let plain_link = links.iter().copied().find(|&a| !self.looks_like_button(a));
            let link = video_link.or(title_link).or(picture_link).or(plain_link).or(links.first().copied());
            it.href = link.and_then(|a| d.attr(a, "href")).and_then(|h| self.href(h)).unwrap_or_default();
            if let Some(&img) = imgs.first() {
                if let Some(src) = self.img_url(img).and_then(|u| self.asset(&u)) {
                    it.image = src;
                    it.alt = collapse(d.attr(img, "alt").unwrap_or(""));
                }
            }
            let mut meta: Vec<String> = Vec::new();
            for (n, t) in &leaves {
                let plain = t.trim();
                if plain == it.title || plain.chars().all(|c| !c.is_alphanumeric()) || title_node.is_some_and(|tn| d.within(*n, tn)) {
                    continue;
                }
                if it.badge.is_empty() && is_duration(plain) {
                    it.badge = plain.to_string();
                    continue;
                }
                if it.price.is_empty() && is_price(plain) {
                    it.price = plain.to_string();
                    continue;
                }
                if it.tag.is_empty() && class_has(&d.classes(*n), &["categ", "brand"]) {
                    it.tag = plain.to_string();
                }
                if words(plain) <= 12 && !meta.iter().any(|m| m == plain) && meta.len() < 4 && plain.len() < 120 {
                    meta.push(plain.to_string());
                }
            }
            it.meta = meta.into_iter().map(|m| md_escape(&m)).collect::<Vec<_>>().join(" · ");
            let low = it.meta.to_lowercase();
            if !it.badge.is_empty()
                || !yt_id(&it.href).is_empty()
                || vimeo_id(&it.href).is_some()
                || ["views", "view", "zhlédnutí", "aufrufe", "vues", "visualizaciones"].iter().any(|w| low.split(|c: char| !c.is_alphanumeric()).any(|x| x == *w))
            {
                videoish += 1;
            }
            if !it.price.is_empty() {
                priced += 1;
            }
            it.title = md_escape(&it.title);
            it.badge = md_escape(&it.badge);
            it.price = md_escape(&it.price);
            items.push((k, it));
        }
        let n = items.len();
        let pictured = items.iter().filter(|(_, m)| !m.image.is_empty()).count();
        if n < 3 || n * 4 < kids.len() * 3 || pictured * 4 < n * 3 || (videoish * 4 < n * 3 && priced * 5 < n * 3) {
            return None;
        }
        // the categories, when each product names one
        let tags: Vec<String> = items.iter().map(|(_, m)| md_plain(&m.tag)).filter(|t| !t.is_empty()).collect();
        let mut cats: Vec<String> = Vec::new();
        for t in &tags {
            if !cats.contains(t) {
                cats.push(t.clone());
            }
        }
        if tags.len() * 4 < n * 3 || cats.len() < 2 || cats.len() > 8 {
            cats.clear();
        }
        let videos = items.iter().filter(|(_, m)| !yt_id(&m.href).is_empty() || vimeo_id(&m.href).is_some()).count();
        self.videos += videos;
        self.leftover = rest;
        let cols = if n <= 4 { n } else { 4 };
        Some(Item::Media { cols, items: items.into_iter().map(|(_, m)| m).collect(), cats })
    }

    /// A search box goes right before the grid it filters; a site's own search (which needs a
    /// server) becomes a search over the biggest grid of the page.
    fn place_search(&mut self, page: &mut Page) {
        let grid = |i: &Item| match i {
            Item::Moved { item, .. } => matches!(**item, Item::Cards { .. } | Item::Media { .. } | Item::Gallery(_) | Item::List { .. } | Item::Table { .. }),
            Item::Cards { .. } | Item::Media { .. } | Item::Gallery(_) | Item::List { .. } | Item::Table { .. } => true,
            _ => false,
        };
        let size = |i: &Item| -> usize {
            match i {
                Item::Moved { item, .. } => match &**item {
                    Item::Cards { cards, .. } => cards.len(),
                    Item::Media { items, .. } => items.len(),
                    _ => 0,
                },
                Item::Cards { cards, .. } => cards.len(),
                Item::Media { items, .. } => items.len(),
                _ => 0,
            }
        };
        let mut placed = false;
        for s in &mut page.sections {
            let mut i = 0;
            while i < s.items.len() {
                if matches!(s.items[i], Item::Search(_)) {
                    match s.items[i + 1..].iter().position(grid) {
                        Some(off) => {
                            let it = s.items.remove(i);
                            s.items.insert(i + off, it);
                            placed = true;
                            i += off + 2;
                            continue;
                        }
                        None => {
                            s.items.remove(i);
                            self.count("search boxes with nothing to filter");
                            continue;
                        }
                    }
                }
                i += 1;
            }
        }
        if placed || !self.server.iter().any(|f| f == "search over the whole site") {
            return;
        }
        // the biggest grid of six or more
        let mut best: Option<(usize, usize, usize)> = None;
        for (si, s) in page.sections.iter().enumerate() {
            for (ii, it) in s.items.iter().enumerate() {
                let n = size(it);
                if n >= 6 && best.is_none_or(|(_, _, b)| n > b) {
                    best = Some((si, ii, n));
                }
            }
        }
        if let Some((si, ii, _)) = best {
            page.sections[si].items.insert(ii, Item::Search(String::new()));
        }
    }
}

/// The note in the program for a feature that needs a server.
fn todo_text(feature: &str) -> String {
    match feature {
        "sign-in and accounts" => "sign-in and accounts need a server (users, sessions); the links still go to the original site".into(),
        "subscriptions and likes" => "subscriptions, likes and follows need accounts and a database on a server; the counts here are a static copy".into(),
        "search over the whole site" => "a search over the whole site needs a server with an index; the search box here filters this page only".into(),
        "comments" => "comments need a server (accounts, a database, moderation); below is a static copy of the comments shown".into(),
        "recommendations" => "recommendations are chosen per visitor by a server; this is a static copy of the list shown".into(),
        "a cart and payments" => "a cart, orders and payments need a server (stock, orders, a payment provider); prices here are a static copy".into(),
        "uploads" => "uploads need a server that receives and stores the files".into(),
        "video streaming" => "this video is streamed by the original site's server; host the file (or a YouTube or Vimeo copy) to play it here".into(),
        f => format!("{f} needs a server"),
    }
}

/// A label of a side navigation link -> a Lucide icon of the kit.
fn side_icon(label: &str) -> Option<&'static str> {
    let l = label.to_lowercase();
    let table: &[(&[&str], &str)] = &[
        (&["home", "domů", "start"], "house"),
        (&["history", "historie", "verlauf"], "history"),
        (&["music", "hudba", "musik", "youtube music"], "music"),
        (&["films", "movies", "film", "filmy", "movies & tv"], "film"),
        (&["hype", "trending now"], "flame"),
        (&["show more", "more", "více"], "chevron-down"),
        (&["show less", "show fewer"], "chevron-up"),
        (&["channels", "creators", "community"], "users"),
        (&["youtube studio", "studio"], "video"),
        (&["settings", "nastavení", "einstellungen"], "settings"),
        (&["help", "nápověda", "hilfe", "faq"], "help-circle"),
        (&["send feedback", "feedback", "zpětná vazba"], "message-square"),
        (&["report history", "report"], "flag"),
        (&["you", "profile", "account", "your channel", "my account"], "user"),
        (&["subscriptions", "odběry"], "tv"),
        (&["shorts", "videos", "videa", "live", "video"], "video"),
        (&["library", "knihovna", "playlists", "docs", "documentation"], "book-open"),
        (&["trending", "explore", "popular"], "trending-up"),
        (&["news", "zprávy", "blog"], "file-text"),
        (&["sport", "sports"], "trophy"),
        (&["learning", "courses", "education"], "graduation-cap"),
        (&["podcasts", "podcast"], "mic"),
        (&["shopping", "shop", "store", "obchod"], "shopping-bag"),
        (&["gaming", "games"], "zap"),
        (&["fashion & beauty", "fashion"], "sparkles"),
        (&["youtube premium", "premium"], "crown"),
        (&["youtube kids", "kids"], "baby"),
        (&["watch later", "later"], "clock"),
        (&["liked videos", "liked", "favorites", "favourites"], "thumbs-up"),
        (&["downloads", "download"], "download"),
        (&["search"], "search"),
        (&["contact", "kontakt"], "mail"),
        (&["about", "o nás"], "info"),
    ];
    table.iter().find(|(names, _)| names.contains(&l.as_str())).map(|(_, i)| *i)
}

/// The id of a YouTube video address (watch?v=, youtu.be/, /embed/, /shorts/, /live/), or "".
pub fn yt_id(url: &str) -> String {
    let u = url.to_ascii_lowercase();
    if !u.starts_with("https://") && !u.starts_with("http://") {
        return String::new();
    }
    let host_ok = u.contains("youtube.com/") || u.contains("youtube-nocookie.com/") || u.contains("youtu.be/");
    if !host_ok {
        return String::new();
    }
    let mut rest = "";
    if let Some(at) = u.find("youtu.be/") {
        rest = &url[at + 9..];
    } else {
        for key in ["?v=", "&v=", "/embed/", "/shorts/", "/live/"] {
            if let Some(at) = u.find(key) {
                rest = &url[at + key.len()..];
                break;
            }
        }
    }
    let id: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    if (6..=20).contains(&id.len()) {
        id
    } else {
        String::new()
    }
}

/// The number of a Vimeo video address (`vimeo.com/123`, `player.vimeo.com/video/123`).
pub fn vimeo_id(url: &str) -> Option<String> {
    let u = url.to_ascii_lowercase();
    let at = u.find("vimeo.com/")?;
    let tail = u[at + 10..].trim_start_matches("video/");
    let id: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
    (id.len() >= 4).then_some(id)
}

/// "4:12", "1:02:33": the length of a video.
fn is_duration(t: &str) -> bool {
    let parts: Vec<&str> = t.split(':').collect();
    (2..=3).contains(&parts.len())
        && parts.iter().all(|p| !p.is_empty() && p.len() <= 2 && p.chars().all(|c| c.is_ascii_digit()))
        && parts[1..].iter().all(|p| p.len() == 2)
}

/// "£51.77", "$10", "1 299 Kč", "19,90 €", "EUR 20": a price.
fn is_price(t: &str) -> bool {
    let t = t.trim();
    if t.chars().count() > 24 || !t.chars().any(|c| c.is_ascii_digit()) || words(t) > 4 {
        return false;
    }
    let l = t.to_lowercase();
    ['£', '$', '€', '¥', '₹', '₽'].iter().any(|c| t.contains(*c))
        || [" kč", "kč", " czk", " eur", "eur ", " usd", "usd ", " zł", " kr", " chf", " gbp"].iter().any(|w| l.ends_with(w.trim_start()) || l.starts_with(w.trim_end()))
}

// ---- free helpers -----------------------------------------------------------------------------------

pub(super) fn plain(id: String, items: Vec<Item>) -> Section {
    Section {
        kind: Kind::Plain,
        id,
        title: String::new(),
        kicker: String::new(),
        text: String::new(),
        bg: String::new(),
        image: String::new(),
        alt: String::new(),
        buttons: Vec::new(),
        items,
        panels: Vec::new(),
        level: 0,
        mo: Mo::default(),
        aside: None,
    }
}

fn is_kicker(i: &Item) -> bool {
    matches!(i, Item::P(t) if words(&md_plain(t)) <= 10)
}

/// Takes a section's title (its first heading near the top), the short line above it (kicker)
/// and a short paragraph after it (the lead text).
fn take_head(s: &mut Section) {
    let pos = s.items.iter().take(3).position(|i| matches!(i, Item::H(..)));
    let Some(pos) = pos else { return };
    // only short lines may stand before the title
    if s.items[..pos].iter().any(|i| !is_kicker(i) && !matches!(i, Item::Todo(_))) {
        return;
    }
    // a heading with an id of its own stays a heading when the section has another id
    if matches!(&s.items[pos], Item::H(_, _, hid) if !hid.is_empty() && !s.id.is_empty() && *hid != s.id) {
        return;
    }
    if let Item::H(level, t, hid) = s.items.remove(pos) {
        s.title = t;
        s.level = level;
        // a heading that links point to gives its section the id
        if !hid.is_empty() && s.id.is_empty() {
            s.id = hid;
        }
    }
    if pos >= 1 && matches!(s.items[pos - 1], Item::P(_)) {
        if let Item::P(k) = s.items.remove(pos - 1) {
            s.kicker = k;
        }
        // any other short line before it stays as text
    }
    // one short paragraph right after the title (not the first of several) is the lead text
    if let Some(Item::P(t)) = s.items.first() {
        if words(&md_plain(t)) <= 45 && !matches!(s.items.get(1), Some(Item::P(_))) {
            s.text = t.clone();
            s.items.remove(0);
        }
    }
}

fn wrap(inner: &str, mark: &str) -> String {
    let t = inner.trim();
    if t.is_empty() || t.contains(mark) || t.starts_with('*') || t.ends_with('*') || t.ends_with('\\') {
        return inner.to_string();
    }
    let lead = if inner.starts_with(char::is_whitespace) { " " } else { "" };
    let tail = if inner.ends_with(char::is_whitespace) { " " } else { "" };
    format!("{lead}{mark}{}{mark}{tail}", collapse(t))
}

/// `[label](url)` alone -> (label, url).
fn link_only(md: &str) -> Option<(String, String)> {
    let t = md.trim();
    if !t.starts_with('[') || !t.ends_with(')') {
        return None;
    }
    let mid = t.find("](")?;
    if t[1..mid].contains("](") || t[mid + 2..].contains("](") {
        return None;
    }
    Some((t[1..mid].to_string(), t[mid + 2..t.len() - 1].to_string()))
}

fn only_links(md: &str) -> bool {
    let mut rest = md.trim();
    if rest.is_empty() {
        return true;
    }
    while let Some(at) = rest.find('[') {
        if !rest[..at].trim().chars().all(|c| c.is_whitespace() || "|·•-–—,/".contains(c)) {
            return false;
        }
        let Some(mid) = rest[at..].find("](") else { return false };
        let Some(end) = rest[at + mid..].find(')') else { return false };
        rest = &rest[at + mid + end + 1..];
    }
    rest.trim().chars().all(|c| c.is_whitespace() || "|·•-–—,/".contains(c))
}

/// "Jana Novak, Brno" -> name "Jana Novak", place "Brno".
fn split_name(q: &mut Quote) {
    if q.place.is_empty() {
        if let Some((n, p)) = q.name.split_once(", ") {
            if words(n) <= 4 && words(p) <= 6 {
                q.place = p.trim().to_string();
                q.name = n.trim().to_string();
            }
        }
    }
}

/// "Read more", "Learn more →", "Details": a link that only leads to the card's page.
fn is_more_link(label: &str) -> bool {
    let l = md_plain(label).to_lowercase();
    ["more", "read", "learn", "details", "view", "see ", "open", "continue", "→", "›", "»", "víc", "více", "detail", "mehr"]
        .iter()
        .any(|w| l.contains(w))
}

fn short_title(t: &str) -> String {
    for sep in [" | ", " – ", " — ", " - ", " · ", ": "] {
        if let Some(at) = t.find(sep) {
            let first = t[..at].trim();
            if !first.is_empty() {
                return first.to_string();
            }
        }
    }
    t.trim().to_string()
}

fn origin(url: &str) -> String {
    match url.find("://") {
        Some(at) => {
            let rest = &url[at + 3..];
            let end = rest.find('/').unwrap_or(rest.len());
            url[..at + 3 + end].to_string()
        }
        None => String::new(),
    }
}

fn unique_id(want: &str, used: &mut HashSet<String>) -> String {
    let base = if want.is_empty() { "section".to_string() } else { want.to_string() };
    let mut id = base.clone();
    let mut k = 2;
    while used.contains(&id) || used.contains(&id.trim_end_matches("-h").to_string()) || id.ends_with("-h") {
        id = format!("{base}-{k}");
        k += 1;
    }
    used.insert(id.clone());
    used.insert(format!("{id}-h"));
    id
}

/// An id usable in the clone: letters, digits, `-` and `_`, starting with a letter.
fn slug_id(id: &str) -> String {
    let s: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
    let s = s.trim_matches('-').to_string();
    if s.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
        s
    } else if s.is_empty() {
        "section".into()
    } else {
        format!("s-{s}")
    }
}

/// The largest candidate of a srcset.
fn largest(set: &str) -> Option<String> {
    let mut best: Option<(f64, String)> = None;
    for part in set.split(',') {
        let mut w = part.split_whitespace();
        let Some(url) = w.next() else { continue };
        let size = w.next().and_then(|d| d.trim_end_matches(['w', 'x']).parse::<f64>().ok()).unwrap_or(1.0);
        if best.as_ref().is_none_or(|(b, _)| size > *b) {
            best = Some((size, url.to_string()));
        }
    }
    best.map(|(_, u)| u)
}

fn query_param(url: &str, key: &str) -> Option<String> {
    let q = url.split_once('?')?.1;
    for kv in q.split('&') {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        if k == key {
            return Some(percent_decode(v));
        }
    }
    None
}

/// "1,200", "98%", "24/7", "3x", "500+", "$10k", "4.9", "★ 4.8"
fn is_number(t: &str) -> bool {
    let t = t.trim();
    if t.is_empty() || t.chars().count() > 12 {
        return false;
    }
    let digits = t.chars().filter(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return false;
    }
    let other: Vec<char> = t.chars().filter(|c| !c.is_ascii_digit()).collect();
    other.iter().all(|c| ".,+%/×x~≈<>$€£¥★ kKMmBsh-–:".contains(*c)) && other.iter().filter(|c| c.is_alphabetic()).count() <= 2
}

// ---- opening hours ----------------------------------------------------------------------------------

/// "Monday – Friday 8:00–16:00" -> ("Monday – Friday", "8:00–16:00").
fn split_day_label(t: &str) -> (String, String) {
    let at = t.find(|c: char| c.is_ascii_digit()).unwrap_or(t.len());
    let lower = t.to_lowercase();
    // a closed day has no digits: "Sunday closed"
    if at == t.len() {
        for w in CLOSED {
            if let Some(p) = lower.find(w) {
                return (t[..p.min(t.len())].trim().trim_end_matches(':').to_string(), t[p.min(t.len())..].trim().to_string());
            }
        }
    }
    (t[..at].trim().trim_end_matches([':', '-', '–']).trim().to_string(), t[at..].trim().to_string())
}

const CLOSED: &[&str] =
    &["closed", "zavřeno", "zavreno", "geschlossen", "fermé", "cerrado", "chiuso", "gesloten", "zamknięte", "nečinné"];

/// Day names (lowercase prefixes) and their numbers, 1 = Monday.
fn day_number(w: &str) -> Option<u32> {
    let w = w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
    if w.is_empty() {
        return None;
    }
    let table: &[(&[&str], u32)] = &[
        (&["monday", "mon", "mo", "pondělí", "pondeli", "po", "montag", "lundi", "lun", "lunes", "lunedì", "poniedziałek", "pon"], 1),
        (
            &[
                "tuesday", "tue", "tues", "tu", "úterý", "utery", "út", "ut", "dienstag", "di", "mardi", "mar", "martes", "martedì",
                "wtorek", "wt",
            ],
            2,
        ),
        (
            &[
                "wednesday",
                "wed",
                "we",
                "středa",
                "streda",
                "st",
                "mittwoch",
                "mi",
                "mercredi",
                "mer",
                "miércoles",
                "mercoledì",
                "środa",
                "śr",
            ],
            3,
        ),
        (
            &[
                "thursday",
                "thu",
                "thur",
                "thurs",
                "th",
                "čtvrtek",
                "ctvrtek",
                "čt",
                "ct",
                "donnerstag",
                "do",
                "jeudi",
                "jeu",
                "jueves",
                "giovedì",
                "czwartek",
                "czw",
            ],
            4,
        ),
        (
            &["friday", "fri", "fr", "pátek", "patek", "pá", "pa", "freitag", "vendredi", "ven", "viernes", "venerdì", "piątek", "pt"],
            5,
        ),
        (&["saturday", "sat", "sa", "sobota", "so", "samstag", "samedi", "sam", "sábado", "sabato", "sob"], 6),
        (
            &["sunday", "sun", "su", "neděle", "nedele", "ne", "sonntag", "dimanche", "dim", "domingo", "domenica", "niedziela", "nd"],
            7,
        ),
    ];
    table.iter().find(|(names, _)| names.contains(&w.as_str())).map(|(_, n)| *n)
}

/// "Monday – Friday" -> "1-5"; "Sat, Sun" -> "6,7"; "Daily" -> "1-7".
fn days_of(label: &str) -> Option<String> {
    let l = label.to_lowercase();
    if l.contains("daily") || l.contains("every day") || l.contains("denně") || l.contains("täglich") || l.contains("all week") {
        return Some("1-7".into());
    }
    let mut out = String::new();
    let mut prev: Option<u32> = None;
    let mut range = false;
    let norm = l.replace(['–', '—'], "-").replace(" to ", "-").replace(" až ", "-").replace(" bis ", "-").replace(" - ", "-");
    let mut token = String::new();
    let mut tokens: Vec<String> = Vec::new();
    for c in norm.chars() {
        if c.is_alphabetic() || c == '.' {
            token.push(c);
        } else {
            if !token.is_empty() {
                tokens.push(std::mem::take(&mut token));
            }
            if c == '-' || c == ',' || c == '&' || c == '+' || c == '/' {
                tokens.push(c.to_string());
            }
        }
    }
    if !token.is_empty() {
        tokens.push(token);
    }
    let mut any = false;
    for t in tokens {
        if t == "-" {
            range = prev.is_some();
            continue;
        }
        if t == "," || t == "&" || t == "+" || t == "/" {
            continue;
        }
        let Some(n) = day_number(&t) else {
            if t.len() > 3 && !matches!(t.as_str(), "and" | "und" | "et" | "y" | "a") {
                return None;
            }
            continue;
        };
        any = true;
        if range {
            out.push('-');
            out.push_str(&n.to_string());
            range = false;
        } else {
            if !out.is_empty() {
                out.push(',');
            }
            out.push_str(&n.to_string());
        }
        prev = Some(n);
    }
    any.then_some(out)
}

/// "8:00 – 16:00, 17:00-19:00" -> "8:00-16:00 17:00-19:00"; "9am–5pm" -> "9:00-17:00".
fn times_of(t: &str) -> Option<String> {
    let l = t.to_lowercase().replace(['–', '—'], "-").replace(" to ", "-");
    // the times in order
    let cs: Vec<char> = l.chars().collect();
    let at = |i: usize, s: &str| s.chars().enumerate().all(|(k, c)| cs.get(i + k) == Some(&c));
    let mut times: Vec<String> = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        if !cs[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let mut h = String::new();
        while i < cs.len() && cs[i].is_ascii_digit() {
            h.push(cs[i]);
            i += 1;
        }
        let mut m = "00".to_string();
        if i + 2 < cs.len() + 1
            && (cs.get(i) == Some(&':') || cs.get(i) == Some(&'.'))
            && cs.get(i + 1).is_some_and(|c| c.is_ascii_digit())
        {
            m = cs[i + 1..(i + 3).min(cs.len())].iter().collect();
            i += 3;
        }
        while cs.get(i) == Some(&' ') {
            i += 1;
        }
        let mut hour: u32 = h.parse().ok()?;
        if at(i, "pm") && hour < 12 {
            hour += 12;
        }
        if at(i, "am") && hour == 12 {
            hour = 0;
        }
        if hour > 24 || m.len() != 2 || !m.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        times.push(format!("{hour}:{m}"));
    }
    if times.is_empty() || !times.len().is_multiple_of(2) {
        return None;
    }
    Some(times.chunks(2).map(|p| format!("{}-{}", p[0], p[1])).collect::<Vec<_>>().join(" "))
}

/// Rows of (day label, times) as the kit's opening hours, or `None` if they are not hours.
fn hours_of(rows: &[(String, String)]) -> Option<Vec<(String, String)>> {
    if rows.len() < 2 {
        return None;
    }
    let mut out = Vec::new();
    for (label, value) in rows {
        let days = days_of(&md_plain(label))?;
        let v = md_plain(value).to_lowercase();
        if v.trim().is_empty() || CLOSED.iter().any(|c| v.contains(c)) && !v.chars().any(|c| c.is_ascii_digit()) {
            out.push((label.clone(), String::new()));
            continue;
        }
        let times = times_of(&v)?;
        out.push((label.clone(), format!("{days} {times}")));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping_markdown() {
        assert_eq!(md_escape("a*b [x] :smile: 10:30 C:\\"), "a\\*b \\[x] \\:smile: 10:30 C:\\\\");
        assert_eq!(md_plain("**Hi** [there](https://x.y) \\*"), "Hi there *");
    }

    #[test]
    fn opening_hours() {
        let rows = vec![
            ("Mon – Fri".to_string(), "8:00 – 16:00".to_string()),
            ("Saturday".into(), "9am – 12pm".into()),
            ("Sunday".into(), "Closed".into()),
        ];
        let h = hours_of(&rows).unwrap();
        assert_eq!(h[0].1, "1-5 8:00-16:00");
        assert_eq!(h[1].1, "6 9:00-12:00");
        assert_eq!(h[2].1, "");
        assert!(hours_of(&[("Price".into(), "10".into()), ("Size".into(), "2".into())]).is_none());
        assert_eq!(days_of("Po – Pá").as_deref(), Some("1-5"));
    }

    #[test]
    fn numbers() {
        for n in ["1,200", "98%", "24/7", "500+", "4.9", "3x", "$10k"] {
            assert!(is_number(n), "{n}");
        }
        for n in ["Hello", "2024 was great year", ""] {
            assert!(!is_number(n), "{n}");
        }
    }
}
