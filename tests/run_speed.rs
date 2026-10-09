//! `nyra run` compiles the generated C with -O1 (fast to compile), `run --release` and `build` with
//! -O2; the build cache keeps one executable per level; a string or array built with `s = s + x`
//! grows in place (no quadratic time).

mod common;

use common::{nyra, scratch, stderr, stdout};
use std::path::Path;
use std::process::Output;
use std::time::Instant;

fn has_cc() -> bool {
    std::env::var("NYRA_CC").is_ok()
        || ["gcc", "clang", "cc", "tcc"].iter().any(|c| std::process::Command::new(c).arg("--version").output().is_ok())
}

/// `nyra run --time <args> prog` with its own folder for the build cache (`tmp`).
fn run(dir: &Path, tmp: &Path, extra: &[&str], prog: &str) -> Output {
    nyra()
        .current_dir(dir)
        .env("TEMP", tmp)
        .env("TMP", tmp)
        .env("TMPDIR", tmp)
        .args(["run", "--time"])
        .args(extra)
        .arg(prog)
        .output()
        .unwrap()
}

fn cached(out: &Output) -> bool {
    stderr(out).contains("| cc cached |")
}

#[test]
fn run_and_release_builds_are_cached_apart() {
    if !has_cc() {
        common::missing("no C compiler for the native backend");
        return;
    }
    let dir = scratch("run-speed-cache");
    let tmp = dir.join("cache");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(dir.join("p.nyra"), "fn main() {\n    var t = 0\n    for i in 0..1000 {\n        t += i\n    }\n    print(t)\n}\n")
        .unwrap();

    let a = run(&dir, &tmp, &[], "p.nyra");
    assert_eq!(stdout(&a), "499500\n", "{}", stderr(&a));
    assert!(!cached(&a), "the first run compiles: {}", stderr(&a));
    assert!(cached(&run(&dir, &tmp, &[], "p.nyra")), "the second run uses the cache");

    // -O2 is another build...
    let b = run(&dir, &tmp, &["--release"], "p.nyra");
    assert_eq!(stdout(&b), "499500\n");
    assert!(!cached(&b), "`--release` compiles with other flags: {}", stderr(&b));
    // ...and neither evicts the other
    assert!(cached(&run(&dir, &tmp, &["--release"], "p.nyra")));
    assert!(cached(&run(&dir, &tmp, &[], "p.nyra")));

    // `build` is -O2 as well, and shares its cache entry with `run --release`
    let out = dir.join("p-built");
    let built = nyra()
        .current_dir(&dir)
        .env("TEMP", &tmp)
        .env("TMP", &tmp)
        .env("TMPDIR", &tmp)
        .args(["build", "p.nyra", "-o"])
        .arg(&out)
        .output()
        .unwrap();
    assert!(built.status.success(), "{}", stderr(&built));
    assert!(stderr(&built).contains("cc cached"), "{}", stderr(&built));
}

#[test]
fn run_and_release_print_the_same_and_pass_arguments() {
    if !has_cc() {
        common::missing("no C compiler for the native backend");
        return;
    }
    let dir = scratch("run-speed-same");
    let tmp = dir.join("cache");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    // an int overflow is the same runtime error at both levels
    std::fs::write(
        dir.join("o.nyra"),
        "fn main() {\n    var x = 9223372036854775807\n    print(\"before\")\n    x += 1\n    print(x)\n}\n",
    )
    .unwrap();
    for flags in [&[][..], &["--release"][..]] {
        let out = run(&dir, &tmp, flags, "o.nyra");
        assert_eq!(out.status.code(), Some(101), "{flags:?}: {}", stderr(&out));
        assert_eq!(stdout(&out), "before\n");
        let err = stderr(&out);
        assert!(err.contains("runtime error[E0255]") && err.contains("9223372036854775807 + 1") && err.contains(":4:5"), "{err}");
    }
}

#[test]
fn building_a_string_with_plus_is_linear() {
    if !has_cc() {
        common::missing("no C compiler for the native backend");
        return;
    }
    let dir = scratch("run-speed-append");
    let tmp = dir.join("cache");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    // 400,000 appends to a string of up to 1.6 MB: 300 GB of copying if every `+` made a new string
    std::fs::write(
        dir.join("s.nyra"),
        "fn main() {\n    var s = \"\"\n    for i in 0..400000 {\n        s = s + \"ab\" + str(i % 10)\n    }\n    print(s.len())\n    var xs: [int] = []\n    for i in 0..200000 {\n        xs = xs + [i]\n    }\n    print(xs.len())\n}\n",
    )
    .unwrap();
    let start = Instant::now();
    let out = run(&dir, &tmp, &[], "s.nyra");
    assert_eq!(stdout(&out), "1200000\n200000\n", "{}", stderr(&out));
    // (the compile is part of this; a quadratic copy would take minutes)
    assert!(start.elapsed().as_secs() < 60, "took {:?}", start.elapsed());
}
