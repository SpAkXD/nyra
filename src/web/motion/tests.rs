use super::*;

fn ok(opt: Opt, v: &str) -> Spec {
    parse(opt, v, &[]).unwrap_or_else(|e| panic!("{v}: {}", e.message))
}

fn err(opt: Opt, v: &str) -> WordError {
    parse(opt, v, &[]).expect_err(v)
}

#[test]
fn every_word_has_its_part_and_every_part_is_known() {
    let names: Vec<&str> = parts(CSS).iter().map(|(n, _)| *n).collect();
    for o in Opt::ALL {
        for w in o.words() {
            let part = format!("{} {w}", o.name());
            assert!(names.contains(&part.as_str()), "motion.css has no part `{part}`");
        }
    }
    for t in TRANSITIONS.iter().filter(|t| **t != "none") {
        assert!(names.contains(&format!("transition {t}").as_str()), "{t}");
    }
    for n in &names {
        let known = ["base", "mods", "reveal", "transition", "ease spring", "ease bounce", "ease snappy"].contains(n)
            || n.strip_prefix("transition ").is_some_and(|t| TRANSITIONS.contains(&t))
            || n.split_once(' ').is_some_and(|(o, w)| Opt::from_name(o).is_some_and(|o| o.words().contains(&w)));
        assert!(known, "motion.css has a part `{n}` that no word asks for");
    }
    let js: Vec<&str> = parts(JS).iter().map(|(n, _)| *n).collect();
    assert_eq!(js, ["core", "words", "count", "draw", "tilt", "magnetic", "marquee", "scroll", "end"]);
}

#[test]
fn the_easings_in_motion_css_are_what_ease_rs_computes() {
    for name in ["spring", "bounce", "snappy"] {
        let part = parts(CSS).into_iter().find(|(n, _)| *n == format!("ease {name}")).expect(name).1;
        assert_eq!(plain(part), easing_part(name), "copy the expected text into the part `ease {name}` of src/web/motion.css");
    }
}

#[test]
fn css_braces_balance_in_every_part() {
    for (name, body) in parts(CSS) {
        let text = strip_css(body);
        let (open, close) = (text.matches('{').count(), text.matches('}').count());
        assert_eq!(open, close, "part `{name}`");
        assert!(!text.contains("EASE"), "part `{name}` has a placeholder");
    }
}

#[test]
fn values_are_read() {
    let s = ok(Opt::Anim, "rise slow 0.2");
    assert_eq!(s.words, ["rise"]);
    assert_eq!(s.speed.as_deref(), Some("slow"));
    assert_eq!(s.number, Some(0.2));
    assert!(s.needs_rule());
    let s = ok(Opt::Hover, "lift spring");
    assert_eq!(s.ease_text.as_deref(), Some("spring"));
    assert!(!s.needs_rule(), "a preset needs no rule of its own");
    let s = ok(Opt::Hover, "lift spring 260 16");
    assert_eq!(s.ease_text.as_deref(), Some("spring 260 16"));
    assert!(matches!(s.ease, Some(Ease::Spring(sp)) if sp.stiffness == 260.0 && sp.damping == 16.0));
    assert!(s.needs_rule());
    let s = ok(Opt::Loop, "bob 4s in-out");
    assert_eq!(s.duration, Some(4.0));
    assert_eq!(ok(Opt::Loop, "spin 600ms").duration, Some(0.6));
    assert_eq!(ok(Opt::Scroll, "parallax 0.3").number, Some(0.3));
    assert_eq!(ok(Opt::Scroll, "parallax -0.2").number, Some(-0.2));
    assert_eq!(ok(Opt::Anim, "stagger zoom").words, ["stagger", "zoom"]);
    assert_eq!(ok(Opt::Hover, "lift shine").words, ["lift", "shine"]);
    assert_eq!(ok(Opt::Anim, "  rise  ").words, ["rise"]);
    for o in Opt::ALL {
        for w in o.words() {
            ok(o, w);
        }
    }
}

#[test]
fn wrong_values_say_what_is_wrong_and_the_nearest_word() {
    let e = err(Opt::Hover, "lfit");
    assert_eq!(e.message, "`lfit` is not a word of `hover:`");
    assert_eq!(e.hint().as_deref(), Some("did you mean `lift`?"));
    let e = err(Opt::Anim, "rise slwo");
    assert_eq!(e.token, "slwo");
    assert_eq!(e.nearest.as_deref(), Some("slow"));
    let e = err(Opt::Hover, "rise");
    assert_eq!(e.message, "`rise` is a word of `anim:`, not of `hover:`");
    let e = err(Opt::Anim, "parallax");
    assert!(e.message.contains("`scroll:`"), "{}", e.message);
    assert!(err(Opt::Anim, "").message.contains("is empty"));
    assert!(err(Opt::Anim, "slow").message.contains("no word"));
    assert!(err(Opt::Anim, "rise rise").message.contains("twice"));
    assert!(err(Opt::Anim, "rise fast slow").message.contains("two speeds"));
    assert!(err(Opt::Anim, "rise out spring").message.contains("two easings"));
    assert!(err(Opt::Anim, "rise 0.2 0.3").message.contains("two numbers"));
    assert!(err(Opt::Anim, "rise 99").message.contains("from 0 to 30"));
    assert!(err(Opt::Scroll, "parallax 3").message.contains("rate"));
    assert!(err(Opt::Loop, "spin 100s").message.contains("duration"));
    assert!(err(Opt::Hover, "lift spring 260").message.contains("two numbers"));
    assert!(err(Opt::Hover, "lift spring 100 0.6").message.contains("to settle"));
    assert!(err(Opt::Anim, "none rise").message.contains("stands alone"));
}

#[test]
fn keyframes_words_work_in_anim_and_loop() {
    let custom = ["sway"];
    assert_eq!(parse(Opt::Loop, "sway 4s in-out", &custom).unwrap().words, ["sway"]);
    assert_eq!(parse(Opt::Anim, "sway slow", &custom).unwrap().words, ["sway"]);
    let e = parse(Opt::Hover, "sway", &custom).unwrap_err();
    assert!(e.message.contains("`anim:` and `loop:`"), "{}", e.message);
    let e = parse(Opt::Loop, "swya", &custom).unwrap_err();
    assert_eq!(e.nearest.as_deref(), Some("sway"));
}

#[test]
fn keyframes_are_checked_and_normalized() {
    let k = check_keyframes("sway", "0%, 100% { rotate: -3deg } 50% { rotate: 3deg }").unwrap();
    assert_eq!(k.css(), "@keyframes sway{0%,100%{rotate:-3deg}50%{rotate:3deg}}");
    assert!(k.layout.is_empty() && k.layout_warning().is_none());
    let k = check_keyframes("in", "from { opacity: 0; transform: translateY(20px) scale(.9) } to { opacity: 1 }").unwrap();
    assert_eq!(k.css(), "@keyframes in{0%{opacity:0;transform:translateY(20px) scale(.9)}100%{opacity:1}}");
    let k = check_keyframes("glow-2", "50% { --x: 3; box-shadow: 0 0 20px rgb(0 0 0 / .2); -webkit-mask-size: 50%; color: red }");
    assert!(k.is_ok(), "{k:?}");
}

#[test]
fn layout_properties_are_listed_for_the_warning() {
    let k = check_keyframes("grow", "from { width: 0; opacity: 0 } to { width: 100%; margin-top: 4px }").unwrap();
    assert_eq!(k.layout, ["width", "margin-top"]);
    let (msg, hint) = k.layout_warning().unwrap();
    assert_eq!(
        msg,
        "the keyframes `grow` animate the layout properties `width`, `margin-top`: the page is laid out again on every frame"
    );
    assert!(hint.contains("`translate`"), "{hint}");
    let one = check_keyframes("drop", "to { top: 10px }").unwrap();
    assert!(one.layout_warning().unwrap().0.contains("layout property `top`"));
}

#[test]
fn wrong_keyframes_are_refused_with_a_reason() {
    let e = |n: &str, b: &str| check_keyframes(n, b).expect_err(b);
    assert!(e("Sway", "to{opacity:0}").message.contains("cannot name"));
    assert!(e("1up", "to{opacity:0}").message.contains("cannot name"));
    assert!(e("a--b", "to{opacity:0}").message.contains("cannot name"));
    assert!(e("rise", "to{opacity:0}").message.contains("already a word of `anim:`"));
    assert!(e("lift", "to{opacity:0}").message.contains("`hover:`"));
    assert!(e("slow", "to{opacity:0}").message.contains("speed or an easing"));
    assert!(e("sway", "").message.contains("no body"));
    assert!(e("sway", "50% rotate: 3deg").message.contains("not followed by"));
    assert!(e("sway", "50% { rotate: 3deg").message.contains("not closed"));
    assert!(e("sway", "50% { 20% { rotate: 3deg } }").message.contains("not closed before"));
    assert!(e("sway", "half { rotate: 3deg }").message.contains("not an offset"));
    assert!(e("sway", "150% { rotate: 3deg }").message.contains("not an offset"));
    assert!(e("sway", "{ rotate: 3deg }").message.contains("no offset"));
    assert!(e("sway", "50% { }").message.contains("empty"));
    assert!(e("sway", "50% { rotate 3deg }").message.contains("property: value"));
    let typo = e("sway", "50% { opacty: 0 }");
    assert!(typo.message.contains("`opacty` is not a property"), "{}", typo.message);
    assert_eq!(typo.hint.as_deref(), Some("did you mean `opacity`?"));
    assert!(e("sway", "50% { display: none }").hint.unwrap().contains("`transform`"));
    for bad in ["url(x.png)", "\"a\"", "red !important", "rotate(3deg", "a</style>", "x\\61"] {
        let v = format!("50% {{ background: {bad} }}");
        assert!(e("sway", &v).message.contains("not a plain CSS value"), "{bad}");
    }
    assert!(e("sway", "50% { opacity: }").message.contains("no value"));
}

#[test]
fn css_in_a_string_is_text() {
    let (toks, errs) = crate::lexer::lex(
        "keyframes(\"sway\", \"0%, 100% { rotate: -3deg } 50% { rotate: 3deg; opacity: .5 }\")
",
    );
    assert!(errs.is_empty(), "{errs:?}");
    assert_eq!(toks[4].tok, Tok::Str("0%, 100% { rotate: -3deg } 50% { rotate: 3deg; opacity: .5 }".into()));
    // code stays code: no space after the brace, or no `property: value`
    for s in ["\"{x:>8}\"", "\"{ x }\"", "\"{n: >4}\"", "\"{ f(a: 1) }\""] {
        let (toks, _) = crate::lexer::lex(s);
        assert!(matches!(toks[0].tok, Tok::Interp(_)), "{s}: {:?}", toks[0].tok);
    }
}

#[test]
fn lint_finds_wrong_words_and_keyframes_in_literals() {
    let src = r#"use web
keyframes("sway", "0%, 100% { rotate: -3deg } 50% { rotate: 3deg }")
keyframes("grow", "to { width: 100% }")
keyframes("rise", "to { opacity: 1 }")
site("A", transition: "fdae", motion: "calm")
page("/", "A") {
    img("leaf.png", "", loop: "sway 4s in-out", hover: "lfit")
    card("a", "b", anim: "rise slow {d}", morph: "flora", at: 0.5)
    h1("x", anim: "words", at: 99)
    section("x", "y", morph: "9x") { p("z", scroll: "parallax 0.3") }
}
"#;
    let issues = lint_source(src);
    let got: Vec<(IssueKind, usize, String)> = issues.iter().map(|i| (i.kind, i.span.line, i.message.clone())).collect();
    let expect = [
        (
            IssueKind::LayoutProperty,
            3,
            "the keyframes `grow` animate the layout property `width`: the page is laid out again on every frame",
        ),
        (IssueKind::BadKeyframes, 4, "`rise` is already a word of `anim:`; a keyframes word needs a new name"),
        (IssueKind::UnknownWord, 5, "`transition: \"fdae\"` is not one of `fade`, `slide`, `rise`, `none`"),
        (IssueKind::UnknownWord, 7, "`lfit` is not a word of `hover:`"),
        (IssueKind::UnknownWord, 9, "`at: 99` is outside 0 to 30 seconds after the parent's start"),
        (IssueKind::UnknownWord, 10, "`morph: \"9x\"` is not a name: use letters, digits and `-`, starting with a letter"),
    ];
    assert_eq!(got.len(), expect.len(), "{got:#?}");
    for (g, e) in got.iter().zip(expect) {
        assert_eq!((g.0, g.1, g.2.as_str()), e);
    }
    assert_eq!(issues[2].hint.as_deref(), Some("did you mean `fade`?"));
    assert_eq!(issues[3].hint.as_deref(), Some("did you mean `lift`?"));
    // the name is reported at the name, the body at the body
    assert_eq!(issues[1].span.col, 11);
}

#[test]
fn nothing_moving_means_no_css_and_no_js() {
    assert_eq!(motion_css(&[], &[]), "");
    assert_eq!(motion_js(&[]), "");
    assert_eq!(motion_css(&[("anim", "flyy")], &[]), "", "a value that does not parse is left out");
    assert_eq!(motion_css(&[("transition", "none")], &[]), "");
    assert_eq!(motion_css(&[("transition", "fade"), ("motion", "off")], &[]), "");
}

#[test]
fn only_the_parts_used_are_included() {
    let css = motion_css(&[("anim", "rise")], &[]);
    assert!(css.contains("[data-anim~=rise]{--m-y:28px}"));
    assert!(css.contains("@keyframes m-in{"));
    assert!(css.contains("prefers-reduced-motion:reduce"));
    for absent in ["m-word", "data-hover~=lift", "m-marquee", "view-transition", "@keyframes sway", "/*"] {
        assert!(!css.contains(absent), "{absent} in {css}");
    }
    let css = motion_css(&[("hover", "lift"), ("data-loop", "ping")], &[]);
    assert!(css.contains("[data-hover~=lift]:is(:hover,:focus-visible)"));
    assert!(css.contains("@keyframes m-ping"));
    assert!(!css.contains("@keyframes m-in{"), "no reveal without anim words");
    let all: Vec<(&str, &str)> = Opt::ALL.iter().flat_map(|o| o.words().iter().map(|w| (o.name(), *w))).collect();
    let full = motion_css(&all, &[]);
    assert!(full.len() > css.len());
    assert!(full.contains("[data-anim~=draw]") && full.contains("@keyframes m-progress"));
}

#[test]
fn values_with_numbers_get_their_own_rules() {
    let css = motion_css(
        &[
            ("anim", "rise slow 0.2"),
            ("anim", "rise slow 0.2"),
            ("loop", "bob 4s"),
            ("hover", "tilt 1.5"),
            ("scroll", "parallax 0.3"),
            ("at", "0.5"),
            ("at", "1"),
            ("morph", "flora"),
            ("hover", "lift spring 260 16"),
        ],
        &[],
    );
    assert_eq!(css.matches("[data-anim=\"rise slow 0.2\"]{--ma-d:.2s}").count(), 1, "{css}");
    assert!(css.contains("[data-loop=\"bob 4s\"]{--ml-dur:4s}"));
    assert!(css.contains("[data-hover=\"tilt 1.5\"]{--mh-k:1.5}"));
    assert!(css.contains("[data-scroll=\"parallax 0.3\"]{--ms-r:.3}"));
    assert!(css.contains("[data-at=\"0.5\"]{--m-at:.5s}") && css.contains("[data-at=\"1\"]{--m-at:1s}"));
    assert!(css.contains("[data-morph=\"flora\"]{view-transition-name:m-flora}"));
    let spring = Spring::new(260.0, 16.0).curve();
    assert!(css.contains(&format!("[data-hover=\"lift spring 260 16\"]{{--mh-e:{};--mh-dur:", spring.bezier_css())));
    assert!(css.contains(&format!("{{[data-hover=\"lift spring 260 16\"]{{--mh-e:{}}}}}", spring.linear())));
}

#[test]
fn keyframes_words_get_their_css_when_used() {
    let k = check_keyframes("sway", "0%, 100% { rotate: -3deg } 50% { rotate: 3deg }").unwrap();
    let unused = motion_css(&[("anim", "rise")], std::slice::from_ref(&k));
    // defined but not used in an option: the keyframes are there (raw `css(...)` may use them), no rules
    assert!(unused.contains("@keyframes sway{") && !unused.contains("[data-anim~=sway]"));
    let css = motion_css(&[("loop", "sway 4s in-out")], std::slice::from_ref(&k));
    assert!(css.contains("@keyframes sway{0%,100%{rotate:-3deg}50%{rotate:3deg}}"));
    assert!(css.contains("[data-loop~=sway]{--m-a2:sway"));
    assert!(css.contains("[data-loop=\"sway 4s in-out\"]{--ml-dur:4s}"));
    assert!(!css.contains("[data-anim~=sway]"));
    let css = motion_css(&[("anim", "sway")], &[k]);
    assert!(css.contains("[data-anim~=sway].m-in{--m-a1:sway"));
    assert!(css.contains("paused both"));
}

#[test]
fn page_transitions() {
    let css = motion_css(&[("transition", "slide")], &[]);
    assert!(css.contains("@view-transition{navigation:auto}"));
    assert!(css.contains("m-vt-slide-in") && !css.contains("m-vt-rise-in"));
    assert!(css.contains("@media (prefers-reduced-motion:reduce){@view-transition{navigation:none}}"));
}

#[test]
fn the_script_has_only_what_the_words_need() {
    assert_eq!(motion_js(&[("hover", "lift glow shine"), ("loop", "bob ping")]), "");
    let js = motion_js(&[("anim", "rise")]);
    assert!(js.starts_with("(()=>{") && js.trim_end().ends_with("})();"), "{js}");
    assert!(!js.contains("m-wi") && !js.contains("m-glare") && !js.contains("//"));
    let js = motion_js(&[("anim", "words"), ("hover", "tilt magnetic")]);
    assert!(js.contains("m-wi") && js.contains("m-glare") && js.contains("170*"));
    let js = motion_js(&[("scroll", "parallax")]);
    assert!(js.contains("animation-timeline: view()"), "the scroll fallback");
}

use crate::web::gzip::gzip_size;

#[test]
fn the_script_of_a_typical_page_stays_within_its_budget() {
    // the flora page: sections rise, headings by words, a staggered grid, counters, a hero sequence
    let typical = [("anim", "rise"), ("anim", "words"), ("anim", "stagger"), ("anim", "count"), ("at", "0.5")];
    let js = motion_js(&typical);
    let gz = gzip_size(js.as_bytes());
    assert!(gz <= 1536, "motion.js for a typical page is {gz} bytes gzipped, over the 1.5 KB budget");
    // everything at once still stays small
    let all: Vec<(&str, &str)> = Opt::ALL.iter().flat_map(|o| o.words().iter().map(|w| (o.name(), *w))).collect();
    let gz_all = gzip_size(motion_js(&all).as_bytes());
    assert!(gz_all <= 3072, "all of motion.js is {gz_all} bytes gzipped");
    let css_gz = gzip_size(motion_css(&typical, &[]).as_bytes());
    assert!(css_gz <= 3072, "the motion CSS of a typical page is {css_gz} bytes gzipped");
    eprintln!("typical page: js {} B ({gz} gz), css {css_gz} gz; all js {gz_all} gz", js.len());
}

#[test]
fn html_is_scanned_for_motion_attributes() {
    let html = r##"<section data-anim="rise slow 0.2"><h2 data-anim='words' data-at="0.5">A</h2><a data-hover=lift href="#">x</a><div data-loop="marquee"></div><p data-anim="rise slow 0.2"></p><img data-morph="flora" data-scroll="parallax"></section><html data-motion="calm"><div data-other="x">"##;
    let mut out = Vec::new();
    scan_html(html, &mut out);
    let got: Vec<(&str, &str)> = out.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    assert_eq!(
        got,
        [
            ("anim", "rise slow 0.2"),
            ("anim", "words"),
            ("at", "0.5"),
            ("hover", "lift"),
            ("loop", "marquee"),
            ("morph", "flora"),
            ("scroll", "parallax"),
            ("motion", "calm")
        ]
    );
}

#[test]
fn unsafe_values_never_reach_a_selector() {
    // a value the renderer would never write, but the build might be handed
    let css = motion_css(&[("anim", "rise\"]{x}"), ("morph", "a\"b"), ("at", "1;x")], &[]);
    assert!(!css.contains("{x}") && !css.contains("a\"b") && !css.contains("1;x"), "{css}");
}

/// `MOTION_DUMP=dir cargo test --release dump_motion -- --ignored` writes the assembled files for
/// a look (and for the fixtures' size notes).
#[test]
#[ignore]
fn dump_motion() {
    let dir = std::env::var("MOTION_DUMP").expect("MOTION_DUMP");
    let typical = [("anim", "rise"), ("anim", "words"), ("anim", "stagger"), ("anim", "count"), ("at", "0.5")];
    let all: Vec<(&str, &str)> = Opt::ALL.iter().flat_map(|o| o.words().iter().map(|w| (o.name(), *w))).collect();
    std::fs::write(format!("{dir}/typical.js"), motion_js(&typical)).unwrap();
    std::fs::write(format!("{dir}/typical.css"), motion_css(&typical, &[])).unwrap();
    std::fs::write(format!("{dir}/all.js"), motion_js(&all)).unwrap();
    std::fs::write(format!("{dir}/all.css"), motion_css(&all, &[])).unwrap();
    let eases: String = ["spring", "bounce", "snappy"].iter().map(|n| easing_part(n)).collect();
    std::fs::write(format!("{dir}/easings.css"), eases).unwrap();
}
