//! `nyra upgrade`: installs the newest release over the running `nyra`.
//!
//! It asks GitHub's releases API for the newest version (through the system's `curl`: Nyra has no
//! TLS code of its own), downloads the archive for this platform and SHA256SUMS.txt, checks the
//! archive's SHA-256, unpacks it with the system's `tar`, checks that the new executable starts, and
//! only then swaps it in. On Windows a running executable cannot be overwritten but can be renamed:
//! the old one is moved aside to `nyra.exe.old`, which the next upgrade removes.
//!
//! `NYRA_RELEASES_URL` replaces the API address (`.../releases`, which has `/latest` and
//! `/tags/<tag>`), so the tests serve a release from a local server and never use the network.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use crate::json::Json;

const RELEASES: &str = "https://api.github.com/repos/SpAkXD/nyra/releases";
const VERSION: &str = env!("CARGO_PKG_VERSION");

const USAGE: &str = "\
usage: nyra upgrade [--check] [--version X] [--force]

Installs the newest Nyra release over this executable, after checking its SHA-256.
  --check       only say whether a newer release exists
  --version X   install version X (e.g. 0.8.0), newer or older
  --force       install even when this version is already the newest
Needs curl and tar (both come with Windows 10 and later, macOS and Linux).
";

/// The archive name's platform part, for the platforms that have release builds.
fn target() -> Option<&'static str> {
    if cfg!(all(windows, target_arch = "x86_64")) {
        Some("x86_64-pc-windows-msvc")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("x86_64-unknown-linux-gnu")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("aarch64-apple-darwin")
    } else {
        None
    }
}

const EXE: &str = if cfg!(windows) { "nyra.exe" } else { "nyra" };
const ARCHIVE: &str = if cfg!(windows) { "zip" } else { "tar.gz" };

pub fn run(args: Vec<String>) -> ExitCode {
    let mut check = false;
    let mut force = false;
    let mut want: Option<String> = None;
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--check" => check = true,
            "--force" => force = true,
            "--version" => match it.next() {
                Some(v) => want = Some(v),
                None => return fail("--version needs a version, e.g. `nyra upgrade --version 0.8.0`"),
            },
            "-h" | "--help" | "help" => {
                print!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            _ if !a.starts_with('-') && want.is_none() && parse_version(&a).is_some() => want = Some(a),
            _ => return fail(&format!("unknown argument `{a}`\n\n{USAGE}")),
        }
    }
    match upgrade(check, force, want.as_deref()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(&e),
    }
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("nyra upgrade: {msg}");
    ExitCode::from(1)
}

fn upgrade(check: bool, force: bool, want: Option<&str>) -> Result<(), String> {
    // the file itself, not a symlink to it (on Windows canonical paths start with `\\?\`, and links are rare)
    let exe = std::env::current_exe()
        .and_then(|p| if cfg!(windows) { Ok(p) } else { p.canonicalize() })
        .map_err(|e| format!("cannot find this executable: {e}"))?;
    remove_old(&exe);
    let base = std::env::var("NYRA_RELEASES_URL").unwrap_or_else(|_| RELEASES.to_string());
    let base = base.trim_end_matches('/');
    let url = match want {
        Some(v) => format!("{base}/tags/{}", tag_of(v)),
        None => format!("{base}/latest"),
    };
    let body = fetch(&url).map_err(|e| match want {
        Some(v) => format!("cannot find release {}: {e}", tag_of(v)),
        None => format!("cannot ask for the newest release: {e}"),
    })?;
    let rel = Release::parse(&body)?;
    let current = parse_version(VERSION).ok_or("this build has no version")?;
    let latest = parse_version(&rel.tag).ok_or_else(|| format!("the release `{}` has no version number", rel.tag))?;
    let newer = latest > current;

    if check {
        if newer {
            println!("nyra {VERSION} is installed; {} is available. Run `nyra upgrade` to install it.", rel.tag);
        } else if latest == current {
            println!("nyra {VERSION} is installed, the newest release.");
        } else {
            println!("nyra {VERSION} is installed, newer than the newest release ({}).", rel.tag);
        }
        return Ok(());
    }
    if latest == current && !force {
        println!("nyra {VERSION} is already installed (`--force` installs it again).");
        return Ok(());
    }
    if want.is_none() && !newer && !force {
        println!("nyra {VERSION} is newer than the newest release ({}): nothing to do.", rel.tag);
        return Ok(());
    }
    if let Some(manager) = package_manager(&exe) {
        return Err(format!("this nyra ({}) was installed by {manager}", exe.display()));
    }
    let Some(target) = target() else {
        return Err(format!(
            "there is no release build for this platform ({}-{}); build it from source: cargo install --git https://github.com/SpAkXD/nyra",
            std::env::consts::ARCH,
            std::env::consts::OS
        ));
    };

    let name = format!("nyra-{}-{target}.{ARCHIVE}", rel.tag);
    let asset = rel.asset(&name).ok_or_else(|| format!("release {} has no {name}", rel.tag))?;
    let sums = rel
        .asset("SHA256SUMS.txt")
        .ok_or_else(|| format!("release {} has no SHA256SUMS.txt, so its download cannot be checked", rel.tag))?;

    let tmp = std::env::temp_dir().join(format!("nyra-upgrade-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| format!("cannot create {}: {e}", tmp.display()))?;
    let result = install(&rel, target, asset, sums, &tmp, &exe);
    let _ = std::fs::remove_dir_all(&tmp);
    result?;
    println!("nyra upgraded: {VERSION} -> {} ({})", rel.tag.trim_start_matches('v'), exe.display());
    Ok(())
}

/// Downloads `asset`, checks it against `sums` and puts its executable in place of `exe`.
fn install(rel: &Release, target: &str, asset: &Asset, sums: &Asset, tmp: &Path, exe: &Path) -> Result<(), String> {
    let name = &asset.name;
    println!("Downloading {name} ({:.1} MB)", asset.size as f64 / 1_000_000.0);
    let archive = tmp.join(name);
    download(&asset.url, &archive)?;
    let sums = String::from_utf8_lossy(&fetch(&sums.url)?).to_string();
    let expected = sums
        .lines()
        .filter_map(|l| l.split_once(char::is_whitespace))
        .find(|(_, f)| f.trim().trim_start_matches('*') == name.as_str())
        .map(|(h, _)| h.trim().to_ascii_lowercase())
        .ok_or_else(|| format!("SHA256SUMS.txt of {} has no line for {name}", rel.tag))?;
    let data = std::fs::read(&archive).map_err(|e| format!("cannot read {}: {e}", archive.display()))?;
    let actual = hex(&sha256(&data));
    if actual != expected {
        return Err(format!("the SHA-256 of {name} is {actual}, but SHA256SUMS.txt says {expected}; nothing was changed"));
    }
    println!("Checked the SHA-256");

    let out = tmp.join("x");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    unpack(&archive, &out)?;
    let new = out.join(format!("nyra-{}-{target}", rel.tag)).join(EXE);
    if !new.is_file() {
        return Err(format!("{name} has no nyra-{}-{target}/{EXE}", rel.tag));
    }
    // an executable that does not start (another libc, a broken download) must not replace this one
    let said = Command::new(&new).arg("--version").output().map_err(|e| format!("the new nyra does not start: {e}"))?;
    let said = String::from_utf8_lossy(&said.stdout).to_string() + &String::from_utf8_lossy(&said.stderr);
    if !said.starts_with("nyra ") {
        return Err(format!("the new nyra does not start: {}", said.trim()));
    }
    replace(exe, &new)
}

/// Puts `new` where `exe` is. The new file is copied next to the old one first, so the last step is
/// a rename within one folder.
fn replace(exe: &Path, new: &Path) -> Result<(), String> {
    let dir = exe.parent().ok_or("the executable has no folder")?;
    let staged = dir.join(format!(".nyra-upgrade-{}", std::process::id()));
    let denied = |e: std::io::Error| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            format!(
                "cannot write to {}: permission denied (run it as the user who installed nyra, or reinstall with the install script)",
                dir.display()
            )
        } else {
            format!("cannot write to {}: {e}", dir.display())
        }
    };
    std::fs::copy(new, &staged).map_err(denied)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755));
    }
    if cfg!(windows) {
        // Windows: move the running executable aside, then put the new one in its place
        let old = old_path(exe);
        let _ = std::fs::remove_file(&old);
        if let Err(e) = std::fs::rename(exe, &old) {
            let _ = std::fs::remove_file(&staged);
            return Err(format!("cannot move {} aside: {e}", exe.display()));
        }
        if let Err(e) = std::fs::rename(&staged, exe) {
            let _ = std::fs::rename(&old, exe);
            let _ = std::fs::remove_file(&staged);
            return Err(format!("cannot put the new nyra in place: {e}"));
        }
        // works only when this process is not the one that was replaced; otherwise the next upgrade does it
        let _ = std::fs::remove_file(&old);
    } else if let Err(e) = std::fs::rename(&staged, exe) {
        let _ = std::fs::remove_file(&staged);
        return Err(denied(e));
    }
    Ok(())
}

fn old_path(exe: &Path) -> PathBuf {
    let mut s = exe.as_os_str().to_owned();
    s.push(".old");
    PathBuf::from(s)
}

/// Removes what an earlier upgrade could not: the replaced executable and an interrupted copy.
fn remove_old(exe: &Path) {
    let _ = std::fs::remove_file(old_path(exe));
}

/// Set when `exe` belongs to a package manager, which should do the upgrade.
fn package_manager(exe: &Path) -> Option<&'static str> {
    let p = exe.to_string_lossy().replace('\\', "/");
    if p.contains("/Cellar/") {
        Some("Homebrew: run `brew upgrade nyra`")
    } else if p.contains("/WinGet/Packages/") {
        Some("winget: run `winget upgrade SpAkXD.Nyra`")
    } else if p.contains("/.cargo/bin/") {
        Some("cargo: run `cargo install --force --git https://github.com/SpAkXD/nyra`")
    } else {
        None
    }
}

fn tag_of(v: &str) -> String {
    if v.starts_with('v') {
        v.to_string()
    } else {
        format!("v{v}")
    }
}

/// `v0.9.1` or `0.9.1-rc.1` as numbers to compare; a pre-release sorts before its release.
fn parse_version(s: &str) -> Option<(u64, u64, u64, bool)> {
    let s = s.strip_prefix('v').unwrap_or(s);
    let (core, pre) = match s.split_once('-') {
        Some((c, _)) => (c, true),
        None => (s, false),
    };
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    let v = (parts.next()??, parts.next()??, parts.next().unwrap_or(Some(0))?, !pre);
    if parts.next().is_some() {
        return None;
    }
    Some(v)
}

struct Asset {
    name: String,
    url: String,
    size: u64,
}

struct Release {
    tag: String,
    assets: Vec<Asset>,
}

impl Release {
    fn parse(body: &[u8]) -> Result<Release, String> {
        let text = String::from_utf8_lossy(body);
        let json = Json::parse(&text).map_err(|e| format!("the release answer is not JSON: {e}"))?;
        let tag = json.get("tag_name").and_then(Json::as_str).ok_or("the release answer has no tag_name")?.to_string();
        let assets = json
            .get("assets")
            .and_then(Json::as_array)
            .unwrap_or(&[])
            .iter()
            .filter_map(|a| {
                Some(Asset {
                    name: a.get("name")?.as_str()?.to_string(),
                    url: a.get("browser_download_url")?.as_str()?.to_string(),
                    size: a.get("size").and_then(Json::as_f64).unwrap_or(0.0) as u64,
                })
            })
            .collect();
        Ok(Release { tag, assets })
    }

    fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }
}

fn curl() -> Command {
    let mut c = Command::new("curl");
    c.args(["-fsSL", "--retry", "2", "-H", "Accept: application/vnd.github+json"]);
    c.arg("-A").arg(format!("nyra/{VERSION}"));
    c
}

fn curl_error(e: std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::NotFound {
        "curl is not installed: install it (it comes with Windows 10 and later, and macOS), or reinstall Nyra with its install script"
            .into()
    } else {
        format!("cannot start curl: {e}")
    }
}

/// The body of `url`.
fn fetch(url: &str) -> Result<Vec<u8>, String> {
    let out = curl().arg(url).output().map_err(curl_error)?;
    if !out.status.success() {
        return Err(format!("{url}: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(out.stdout)
}

fn download(url: &str, to: &Path) -> Result<(), String> {
    let out = curl().arg("-o").arg(to).arg(url).output().map_err(curl_error)?;
    if !out.status.success() {
        return Err(format!("cannot download {url}: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(())
}

/// Unpacks a .zip or .tar.gz with the system's tar (on Windows its own bsdtar, which reads zip files;
/// PowerShell's Expand-Archive when that is missing).
fn unpack(archive: &Path, to: &Path) -> Result<(), String> {
    let tar = if cfg!(windows) {
        let sys = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        Path::new(&sys).join("System32").join("tar.exe")
    } else {
        PathBuf::from("tar")
    };
    let flags = if cfg!(windows) { "-xf" } else { "-xzf" };
    let ok = Command::new(&tar).arg(flags).arg(archive).arg("-C").arg(to).output().map(|o| o.status.success()).unwrap_or(false);
    if ok {
        return Ok(());
    }
    if cfg!(windows) {
        let script = format!(
            "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
            archive.display().to_string().replace('\'', "''"),
            to.display().to_string().replace('\'', "''")
        );
        let ok =
            Command::new("powershell").args(["-NoProfile", "-Command", &script]).output().map(|o| o.status.success()).unwrap_or(false);
        if ok {
            return Ok(());
        }
    }
    Err(format!("cannot unpack {} (with {})", archive.display(), tar.display()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// SHA-256 (FIPS 180-4).
pub fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01,
        0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
        0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147,
        0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08,
        0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64).wrapping_mul(8)).to_be_bytes());
    for block in msg.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (i, word) in block.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*word);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 32];
    for (i, x) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&x.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_the_standard_vectors() {
        assert_eq!(hex(&sha256(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(
            hex(&sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        assert_eq!(hex(&sha256(&vec![b'a'; 1_000_000])), "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
    }

    #[test]
    fn versions_compare_as_numbers() {
        assert!(parse_version("v0.10.0") > parse_version("0.9.9"));
        assert!(parse_version("0.9.0-rc.1") < parse_version("0.9.0"));
        assert_eq!(parse_version("v1.2"), Some((1, 2, 0, true)));
        assert_eq!(parse_version("latest"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
    }

    #[test]
    fn package_managers_are_recognized() {
        assert!(package_manager(Path::new("/opt/homebrew/Cellar/nyra/0.9.0/bin/nyra")).is_some());
        assert!(package_manager(Path::new(r"C:\Users\a\AppData\Local\Microsoft\WinGet\Packages\SpAkXD.Nyra_x\nyra.exe")).is_some());
        assert!(package_manager(Path::new("/home/a/.nyra/bin/nyra")).is_none());
    }
}
