//! Getting a page for `nyra clone`. Nyra has no dependencies, so it has no TLS: pages are fetched
//! with the system's `curl` (part of Windows 10 and later, macOS and Linux), with a time limit, a
//! size limit, a browser's User-Agent, following redirects. `--browser` asks an installed Chrome,
//! Edge, Brave or Chromium for the rendered DOM instead (`--headless=new --dump-dom`), for sites
//! that build their page with JavaScript. A local `.html` file is read from disk.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The largest page and stylesheet `clone` reads.
pub const MAX_PAGE: usize = 5 << 20;
pub const MAX_CSS: usize = 2 << 20;
/// At most this many stylesheets (linked and imported) are fetched.
pub const MAX_SHEETS: usize = 12;

const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// A fetched document.
pub struct Fetched {
    pub text: String,
    /// The final URL after redirects (the base for relative links), or the file's path.
    pub url: String,
}

pub fn is_url(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    l.starts_with("http://") || l.starts_with("https://")
}

/// Fetches `url` with curl; `limit` bytes at most.
pub fn get(url: &str, limit: usize, timeout_s: u64) -> Result<Fetched, String> {
    if !is_url(url) {
        return Err(format!("`{url}` is not an http(s) URL"));
    }
    let tmp = std::env::temp_dir().join(format!("nyra-clone-{}-{}.part", std::process::id(), crate::fnv1a(&[url.as_bytes()])));
    let out = Command::new("curl")
        .args(["-sSL", "--compressed", "--max-redirs", "8", "--max-time"])
        .arg(timeout_s.to_string())
        .args(["--max-filesize"])
        .arg(limit.to_string())
        .args(["-A", USER_AGENT, "-H", "Accept: text/html,text/css,*/*;q=0.8", "-H", "Accept-Language: en,*;q=0.5"])
        .args(["-w", "%{url_effective}\n%{http_code}\n%{content_type}", "-o"])
        .arg(&tmp)
        .arg(url)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run `curl` ({e}): nyra clone fetches pages with the system's curl; install it, or save the page and pass the .html file"))?;
    let bytes = std::fs::read(&tmp).unwrap_or_default();
    let _ = std::fs::remove_file(&tmp);
    let info = String::from_utf8_lossy(&out.stdout).to_string();
    let mut lines = info.lines();
    let final_url = lines.next().unwrap_or(url).trim().to_string();
    let code: u32 = lines.next().unwrap_or("0").trim().parse().unwrap_or(0);
    let ctype = lines.next().unwrap_or("").to_ascii_lowercase();
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let why = if out.status.code() == Some(63) { format!("larger than {} MB", limit >> 20) } else { err };
        return Err(format!("cannot fetch {url}: {why}"));
    }
    if code >= 400 {
        return Err(format!("cannot fetch {url}: the server answered HTTP {code}"));
    }
    let bytes = if bytes.len() > limit { bytes[..limit].to_vec() } else { bytes };
    Ok(Fetched { text: decode_bytes(&bytes, &ctype), url: if final_url.is_empty() { url.to_string() } else { final_url } })
}

/// Text from bytes: UTF-8 unless the content type or a `<meta charset>` says a Latin charset.
pub fn decode_bytes(bytes: &[u8], ctype: &str) -> String {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(2048)]).to_ascii_lowercase();
    let latin = |s: &str| s.contains("charset=iso-8859-1") || s.contains("charset=windows-1252") || s.contains("charset=latin1");
    let says_latin = latin(ctype) || latin(&head.replace(['"', '\''], "")) || head.contains("charset=\"iso-8859-1\"");
    match std::str::from_utf8(bytes) {
        Ok(t) if !says_latin => t.trim_start_matches('\u{feff}').to_string(),
        // cut by the size limit in the middle of a character
        Err(e) if !says_latin && e.valid_up_to() + 4 >= bytes.len() => String::from_utf8_lossy(bytes).to_string(),
        _ => bytes.iter().map(|&b| super::dom::cp1252(b)).collect(),
    }
}

/// Reads a local file (a saved page or stylesheet).
pub fn read_file(path: &Path, limit: usize) -> Result<Fetched, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read `{}`: {e}", path.display()))?;
    let bytes = if bytes.len() > limit { &bytes[..limit] } else { &bytes[..] };
    Ok(Fetched { text: decode_bytes(bytes, ""), url: path.display().to_string() })
}

/// The browsers that can dump a rendered page, in the order they are tried.
fn browsers() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = Vec::new();
    if let Ok(b) = std::env::var("NYRA_BROWSER") {
        v.push(PathBuf::from(b));
    }
    if cfg!(windows) {
        for base in ["ProgramFiles", "ProgramFiles(x86)", "LocalAppData"] {
            if let Ok(dir) = std::env::var(base) {
                let d = PathBuf::from(dir);
                v.push(d.join("Google/Chrome/Application/chrome.exe"));
                v.push(d.join("Microsoft/Edge/Application/msedge.exe"));
                v.push(d.join("BraveSoftware/Brave-Browser/Application/brave.exe"));
                v.push(d.join("Chromium/Application/chrome.exe"));
            }
        }
    } else if cfg!(target_os = "macos") {
        for app in [
            "Google Chrome.app/Contents/MacOS/Google Chrome",
            "Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            "Brave Browser.app/Contents/MacOS/Brave Browser",
            "Chromium.app/Contents/MacOS/Chromium",
        ] {
            v.push(PathBuf::from("/Applications").join(app));
        }
    }
    for name in ["google-chrome", "google-chrome-stable", "chromium", "chromium-browser", "microsoft-edge", "brave-browser", "chrome"]
    {
        if let Some(p) = on_path(name) {
            v.push(p);
        }
    }
    v.into_iter().filter(|p| p.is_file()).collect()
}

fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(name)).find(|p| p.is_file())
}

/// The rendered DOM of `url` from a headless browser. The browser gets its own profile folder
/// (kept between runs: a browser's first start in a new profile can stall), with the service
/// workers of earlier runs removed, so a site's offline cache never answers instead of the site.
pub fn rendered(url: &str, timeout_s: u64) -> Result<Fetched, String> {
    let list = browsers();
    let Some(browser) = list.first() else {
        return Err("--browser needs Chrome, Edge, Brave or Chromium installed (or NYRA_BROWSER set to one)".into());
    };
    let profile = std::env::temp_dir().join("nyra-clone-browser");
    let mut last = String::new();
    // First with virtual time, so timers and lazy content settle; a page that animates without
    // end never lets virtual time run out, so then without it. A new profile's first start may
    // also hang on setup: the last try has the profile made.
    for wait in ["--virtual-time-budget=6000", "--timeout=8000", "--timeout=8000"] {
        let _ = std::fs::remove_dir_all(profile.join("Default").join("Service Worker"));
        match dump(browser, &profile, url, wait, timeout_s) {
            Ok(html) if html.contains('<') => {
                return Ok(Fetched { text: html, url: url.to_string() });
            }
            Ok(_) => last = "the browser printed no page".into(),
            Err(e) => last = e,
        }
    }
    Err(format!("{} could not render {url}: {last}", browser.display()))
}

fn dump(browser: &Path, profile: &Path, url: &str, wait: &str, timeout_s: u64) -> Result<String, String> {
    let out = std::env::temp_dir().join(format!("nyra-clone-dom-{}.html", std::process::id()));
    let file = std::fs::File::create(&out).map_err(|e| e.to_string())?;
    let mut child = Command::new(browser)
        .args(["--headless=new", "--disable-gpu", "--no-first-run", "--no-default-browser-check", "--hide-scrollbars", "--mute-audio"])
        .arg(format!("--user-data-dir={}", profile.display()))
        .args(["--window-size=1440,900", wait, "--dump-dom"])
        .arg(url)
        .stdin(Stdio::null())
        .stdout(file)
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot start the browser: {e}"))?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() > Duration::from_secs(timeout_s) => {
                kill_tree(&mut child);
                let _ = std::fs::remove_file(&out);
                return Err(format!("no answer in {timeout_s} s"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(e.to_string()),
        }
    }
    let bytes = std::fs::read(&out).unwrap_or_default();
    let _ = std::fs::remove_file(&out);
    Ok(String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_PAGE)]).to_string())
}

fn kill_tree(child: &mut std::process::Child) {
    if cfg!(windows) {
        let _ = Command::new("taskkill")
            .args(["/F", "/T", "/PID"])
            .arg(child.id().to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}
