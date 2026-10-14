//! The motion fixtures (`tests/web/motion/*.html`) in a headless Chrome: every word reveals and ends
//! in place, counters end on their text, there are no script errors, and with reduced motion (or
//! `data-motion="off"`) nothing animates longer than 10 ms. The browser is `NYRA_CHROME`, else a
//! Chrome or Edge in the usual places; without one (or without Node.js) the test is skipped.

mod common;

use std::path::PathBuf;
use std::process::Command;

fn chrome() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("NYRA_CHROME") {
        return Some(PathBuf::from(p));
    }
    let mut cands: Vec<PathBuf> = [
        "C:/Program Files/Google/Chrome/Application/chrome.exe",
        "C:/Program Files (x86)/Google/Chrome/Application/chrome.exe",
        "C:/Program Files/Microsoft/Edge/Application/msedge.exe",
        "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
        "/usr/bin/google-chrome",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    ]
    .iter()
    .map(PathBuf::from)
    .collect();
    // a Chrome for Testing that Puppeteer downloaded
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        let base = PathBuf::from(home).join(".cache/puppeteer/chrome");
        if let Ok(dirs) = std::fs::read_dir(&base) {
            for d in dirs.flatten() {
                for exe in [
                    "chrome-win64/chrome.exe",
                    "chrome-linux64/chrome",
                    "chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
                ] {
                    cands.push(d.path().join(exe));
                }
            }
        }
    }
    cands.into_iter().find(|p| p.is_file())
}

#[test]
fn motion_fixtures_in_a_browser() {
    if Command::new("node").arg("--version").output().is_err() {
        common::missing("no Node.js to drive the browser for the motion fixtures");
        return;
    }
    let Some(chrome) = chrome() else {
        common::missing("no Chrome or Edge for the motion fixtures (set NYRA_CHROME)");
        return;
    };
    let out = Command::new("node").arg("tests/web/motion/check.mjs").arg(&chrome).output().unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    if out.status.code() == Some(2) {
        common::missing(&format!("the browser did not start: {text}"));
        return;
    }
    assert!(out.status.success(), "{text}");
}
