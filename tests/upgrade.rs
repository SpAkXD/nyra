//! `nyra upgrade` against a release served by a small local server (`NYRA_RELEASES_URL`): no test
//! here touches the network. Each test upgrades a copy of the compiler in a folder of its own.

mod common;

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;

use common::{missing, scratch, stderr, stdout};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const MARKER: &[u8] = b"\nNYRA-UPGRADE-TEST-NEW-BUILD\n";

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

/// A local server for one test: `base` is its address; `start` serves the routes (path -> body)
/// until the test ends.
struct Server {
    listener: TcpListener,
    base: String,
}

impl Server {
    fn new() -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        Server { listener, base }
    }

    fn start(self, routes: HashMap<String, Vec<u8>>) {
        let routes = Arc::new(routes);
        std::thread::spawn(move || {
            for stream in self.listener.incoming() {
                let Ok(mut s) = stream else { continue };
                let routes = routes.clone();
                std::thread::spawn(move || {
                    let mut req = Vec::new();
                    let mut buf = [0u8; 1024];
                    while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                        match s.read(&mut buf) {
                            Ok(0) | Err(_) => return,
                            Ok(n) => req.extend_from_slice(&buf[..n]),
                        }
                    }
                    let head = String::from_utf8_lossy(&req).to_string();
                    let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
                    let reply = match routes.get(&path) {
                        Some(body) => {
                            let mut r =
                                format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes();
                            r.extend_from_slice(body);
                            r
                        }
                        None => b"HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nConnection: close\r\n\r\nnot found".to_vec(),
                    };
                    let _ = s.write_all(&reply);
                });
            }
        });
    }
}

/// The routes of release `tag` for this platform: the API answers, the archive (holding `build` as
/// the executable) and SHA256SUMS.txt (with `sum`, or the archive's real SHA-256).
fn release(base: &str, tag: &str, dir: &Path, build: &[u8], sum: Option<&str>) -> HashMap<String, Vec<u8>> {
    let target = target().unwrap();
    let name = format!("nyra-{tag}-{target}");
    let stage = dir.join("stage");
    let _ = std::fs::remove_dir_all(&stage);
    std::fs::create_dir_all(stage.join(&name)).unwrap();
    let exe = stage.join(&name).join(EXE);
    std::fs::write(&exe, build).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let archive_name = if cfg!(windows) { format!("{name}.zip") } else { format!("{name}.tar.gz") };
    let archive = stage.join(&archive_name);
    let status = if cfg!(windows) {
        // Windows' own tar (bsdtar) writes a zip when the name ends in .zip (-a)
        let tar = Path::new(&std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into())).join(r"System32\tar.exe");
        Command::new(tar).arg("-a").arg("-cf").arg(&archive).arg("-C").arg(&stage).arg(&name).status()
    } else {
        Command::new("tar").arg("-czf").arg(&archive).arg("-C").arg(&stage).arg(&name).status()
    };
    assert!(status.unwrap().success(), "cannot make {archive_name}");
    let data = std::fs::read(&archive).unwrap();
    let sum = sum.map(String::from).unwrap_or_else(|| hex(&sha256(&data)));
    let sums = format!("{}  nyra-{tag}-other.tar.gz\n{sum}  {archive_name}\n", "0".repeat(64));
    let json = format!(
        r#"{{"tag_name":"{tag}","name":"{tag}","prerelease":false,"assets":[
            {{"name":"{archive_name}","size":{},"browser_download_url":"{base}/download/{tag}/{archive_name}"}},
            {{"name":"SHA256SUMS.txt","size":{},"browser_download_url":"{base}/download/{tag}/SHA256SUMS.txt"}}]}}"#,
        data.len(),
        sums.len()
    );
    let mut routes = HashMap::new();
    routes.insert("/releases/latest".to_string(), json.clone().into_bytes());
    routes.insert(format!("/releases/tags/{tag}"), json.into_bytes());
    routes.insert(format!("/download/{tag}/{archive_name}"), data);
    routes.insert(format!("/download/{tag}/SHA256SUMS.txt"), sums.into_bytes());
    routes
}

/// The new build: on Windows this compiler with a marker after it (an .exe still runs with bytes
/// appended); elsewhere a shell script that answers `--version` like nyra.
fn new_build() -> Vec<u8> {
    let mut b = if cfg!(windows) {
        std::fs::read(env!("CARGO_BIN_EXE_nyra")).unwrap()
    } else {
        b"#!/bin/sh\necho \"nyra 99.0.0\"\n#".to_vec()
    };
    b.extend_from_slice(MARKER);
    b
}

/// A folder with a copy of the compiler in `bin/`: the folder and the copy. `None` (skipped) without
/// curl or on a platform that has no release builds.
fn setup(name: &str) -> Option<(PathBuf, PathBuf)> {
    if target().is_none() {
        missing("a platform with release builds");
        return None;
    }
    if Command::new("curl").arg("--version").output().is_err() {
        missing("curl");
        return None;
    }
    let dir = scratch(&format!("upgrade-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    let exe = dir.join("bin").join(EXE);
    std::fs::copy(env!("CARGO_BIN_EXE_nyra"), &exe).unwrap();
    Some((dir, exe))
}

fn upgrade(exe: &Path, base: &str, args: &[&str]) -> Output {
    Command::new(exe)
        .arg("upgrade")
        .args(args)
        .env("NYRA_RELEASES_URL", format!("{base}/releases"))
        .env("NO_PROXY", "127.0.0.1")
        .env("no_proxy", "127.0.0.1")
        .output()
        .unwrap()
}

fn is_new(exe: &Path) -> bool {
    std::fs::read(exe).unwrap().ends_with(MARKER)
}

#[test]
fn check_only_reports_a_newer_release() {
    let Some((dir, exe)) = setup("check") else { return };
    let server = Server::new();
    let base = server.base.clone();
    server.start(release(&base, "v99.0.0", &dir, &new_build(), None));
    let out = upgrade(&exe, &base, &["--check"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains(&format!("nyra {VERSION} is installed; v99.0.0 is available")), "{}", stdout(&out));
    assert!(!is_new(&exe), "--check changed the executable");
}

#[test]
fn upgrade_replaces_the_running_executable() {
    let Some((dir, exe)) = setup("replace") else { return };
    let server = Server::new();
    let base = server.base.clone();
    server.start(release(&base, "v99.0.0", &dir, &new_build(), None));
    let out = upgrade(&exe, &base, &[]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
    assert!(stdout(&out).contains("Checked the SHA-256"), "{}", stdout(&out));
    assert!(stdout(&out).contains(&format!("nyra upgraded: {VERSION} -> 99.0.0")), "{}", stdout(&out));
    assert!(is_new(&exe), "the executable was not replaced");
    // the new build starts
    let v = Command::new(&exe).arg("--version").output().unwrap();
    assert!((stdout(&v) + &stderr(&v)).starts_with("nyra "), "{}{}", stdout(&v), stderr(&v));
    // nothing else is left next to it but the old one that Windows could not delete while it ran
    let left: Vec<String> =
        std::fs::read_dir(dir.join("bin")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
    assert!(left.iter().all(|f| f == EXE || f == &format!("{EXE}.old")), "{left:?}");
}

#[test]
fn a_wrong_checksum_changes_nothing() {
    let Some((dir, exe)) = setup("checksum") else { return };
    let server = Server::new();
    let base = server.base.clone();
    server.start(release(&base, "v99.0.0", &dir, &new_build(), Some(&"ab".repeat(32))));
    let before = std::fs::read(&exe).unwrap();
    let out = upgrade(&exe, &base, &[]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("SHA256SUMS.txt says abab"), "{}", stderr(&out));
    assert!(std::fs::read(&exe).unwrap() == before, "the executable changed");
}

#[test]
fn a_build_that_does_not_start_changes_nothing() {
    let Some((dir, exe)) = setup("broken") else { return };
    let server = Server::new();
    let base = server.base.clone();
    server.start(release(&base, "v99.0.0", &dir, b"not a program", None));
    let before = std::fs::read(&exe).unwrap();
    let out = upgrade(&exe, &base, &[]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("the new nyra does not start"), "{}", stderr(&out));
    assert!(std::fs::read(&exe).unwrap() == before, "the executable changed");
}

#[test]
fn the_same_version_is_up_to_date_and_a_missing_release_is_an_error() {
    let Some((dir, exe)) = setup("same") else { return };
    let server = Server::new();
    let base = server.base.clone();
    let tag = format!("v{VERSION}");
    server.start(release(&base, &tag, &dir, &new_build(), None));
    let out = upgrade(&exe, &base, &[]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("is already installed"), "{}", stdout(&out));
    assert!(!is_new(&exe));
    let out = upgrade(&exe, &base, &["--check"]);
    assert!(stdout(&out).contains("the newest release"), "{}", stdout(&out));
    // --force installs it again; a version that is not released is an error
    let out = upgrade(&exe, &base, &["--force"]);
    assert!(out.status.success() && is_new(&exe), "{}", stderr(&out));
    let out = upgrade(&exe, &base, &["--version", "98.0.0"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("cannot find release v98.0.0"), "{}", stderr(&out));
}

#[test]
fn a_release_without_this_platform_is_an_error() {
    let Some((_, exe)) = setup("noasset") else { return };
    let server = Server::new();
    let base = server.base.clone();
    let json = r#"{"tag_name":"v99.0.0","assets":[{"name":"SHA256SUMS.txt","size":1,"browser_download_url":"x"}]}"#;
    server.start(HashMap::from([("/releases/latest".to_string(), json.as_bytes().to_vec())]));
    let out = upgrade(&exe, &base, &[]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains(&format!("release v99.0.0 has no nyra-v99.0.0-{}", target().unwrap())), "{}", stderr(&out));
}

#[test]
fn version_prints_on_stdout() {
    let out = Command::new(env!("CARGO_BIN_EXE_nyra")).arg("--version").output().unwrap();
    assert!(out.status.success());
    assert_eq!(stdout(&out).trim(), format!("nyra {VERSION}"));
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// SHA-256, as in src/upgrade.rs (the tests cannot reach the binary's code).
fn sha256(data: &[u8]) -> [u8; 32] {
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

#[test]
fn the_test_sha256_is_right() {
    assert_eq!(hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
}
