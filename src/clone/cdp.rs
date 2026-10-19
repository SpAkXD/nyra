//! A small Chrome DevTools Protocol client for `nyra clone --browser` (zero crates): a WebSocket
//! over `std::net` to a headless Chrome, Edge, Brave or Chromium started with
//! `--remote-debugging-port=0`, and the few commands `clone` needs: open a page, wait until its
//! network is quiet, turn down a cookie or consent dialog (the most private choice: "Reject all",
//! never "Accept"), scroll so lazy content loads, and read the DOM.
//!
//! The WebSocket is the plain RFC 6455 framing a client needs: masked text frames out, unmasked
//! frames in (continuations joined, pings answered). The browser listens on 127.0.0.1 only.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::diag::json_str;
use crate::json::Json;

pub struct Ws {
    stream: TcpStream,
    buf: Vec<u8>,
    seed: u64,
}

impl Ws {
    /// Opens `ws://host:port/path`.
    pub fn connect(url: &str) -> Result<Ws, String> {
        let rest = url.strip_prefix("ws://").ok_or_else(|| format!("not a ws:// URL: {url}"))?;
        let (host, path) = rest.split_once('/').map(|(h, p)| (h, format!("/{p}"))).unwrap_or((rest, "/".into()));
        let stream = TcpStream::connect(host).map_err(|e| format!("cannot reach the browser at {host}: {e}"))?;
        stream.set_read_timeout(Some(Duration::from_millis(200))).map_err(|e| e.to_string())?;
        let _ = stream.set_nodelay(true);
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
        let seed = now ^ ((std::process::id() as u64) << 20) ^ 0x9E37_79B9_7F4A_7C15;
        let mut ws = Ws { stream, buf: Vec::new(), seed };
        let key: Vec<u8> = (0..16).map(|_| ws.rand() as u8).collect();
        let req = format!(
            "GET {path} HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {}\r\nSec-WebSocket-Version: 13\r\n\r\n",
            crate::web::digest::base64(&key)
        );
        ws.stream.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
        // the answer's headers
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(end) = ws.buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&ws.buf[..end]).to_string();
                ws.buf.drain(..end + 4);
                if !head.starts_with("HTTP/1.1 101") {
                    return Err(format!("the browser refused the connection: {}", head.lines().next().unwrap_or("")));
                }
                return Ok(ws);
            }
            ws.more(deadline)?;
        }
    }

    fn rand(&mut self) -> u64 {
        // xorshift64*: the masking key only has to vary, not be secret (the socket is local)
        self.seed ^= self.seed >> 12;
        self.seed ^= self.seed << 25;
        self.seed ^= self.seed >> 27;
        self.seed.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Reads what the socket has into the buffer; an error after the deadline.
    fn more(&mut self, deadline: Instant) -> Result<(), String> {
        let mut chunk = [0u8; 64 * 1024];
        loop {
            match self.stream.read(&mut chunk) {
                Ok(0) => return Err("the browser closed the connection".into()),
                Ok(n) => {
                    self.buf.extend_from_slice(&chunk[..n]);
                    return Ok(());
                }
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                    if Instant::now() > deadline {
                        return Err("the browser did not answer in time".into());
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(format!("reading from the browser: {e}")),
            }
        }
    }

    fn need(&mut self, n: usize, deadline: Instant) -> Result<(), String> {
        while self.buf.len() < n {
            self.more(deadline)?;
        }
        Ok(())
    }

    fn send_frame(&mut self, opcode: u8, data: &[u8]) -> Result<(), String> {
        let mut f = Vec::with_capacity(data.len() + 14);
        f.push(0x80 | opcode);
        let n = data.len();
        if n < 126 {
            f.push(0x80 | n as u8);
        } else if n < 65536 {
            f.push(0x80 | 126);
            f.extend_from_slice(&(n as u16).to_be_bytes());
        } else {
            f.push(0x80 | 127);
            f.extend_from_slice(&(n as u64).to_be_bytes());
        }
        let mask = (self.rand() as u32).to_be_bytes();
        f.extend_from_slice(&mask);
        f.extend(data.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        self.stream.write_all(&f).map_err(|e| format!("writing to the browser: {e}"))
    }

    pub fn send(&mut self, text: &str) -> Result<(), String> {
        self.send_frame(1, text.as_bytes())
    }

    /// The next text message, or `None` when nothing came before the deadline.
    pub fn recv(&mut self, deadline: Instant) -> Result<Option<String>, String> {
        let mut message: Vec<u8> = Vec::new();
        loop {
            if self.buf.len() < 2 {
                match self.more(deadline) {
                    Ok(()) => continue,
                    Err(_) if Instant::now() > deadline && message.is_empty() && self.buf.is_empty() => return Ok(None),
                    Err(e) => return Err(e),
                }
            }
            let (b0, b1) = (self.buf[0], self.buf[1]);
            let mut at = 2;
            let mut len = (b1 & 0x7f) as usize;
            if len == 126 {
                self.need(4, deadline + Duration::from_secs(10))?;
                len = u16::from_be_bytes([self.buf[2], self.buf[3]]) as usize;
                at = 4;
            } else if len == 127 {
                self.need(10, deadline + Duration::from_secs(10))?;
                let mut b = [0u8; 8];
                b.copy_from_slice(&self.buf[2..10]);
                len = u64::from_be_bytes(b) as usize;
                at = 10;
            }
            let masked = b1 & 0x80 != 0;
            let key_at = at;
            if masked {
                at += 4;
            }
            if len > 64 << 20 {
                return Err("a message from the browser is too large".into());
            }
            // a frame that has started arriving is read to its end
            self.need(at + len, deadline + Duration::from_secs(30))?;
            let mut payload: Vec<u8> = self.buf[at..at + len].to_vec();
            if masked {
                let key = [self.buf[key_at], self.buf[key_at + 1], self.buf[key_at + 2], self.buf[key_at + 3]];
                payload.iter_mut().enumerate().for_each(|(i, b)| *b ^= key[i % 4]);
            }
            self.buf.drain(..at + len);
            match b0 & 0x0f {
                0..=2 => {
                    message.extend_from_slice(&payload);
                    if b0 & 0x80 != 0 {
                        return Ok(Some(String::from_utf8_lossy(&message).to_string()));
                    }
                }
                8 => return Err("the browser closed the connection".into()),
                9 => self.send_frame(0xA, &payload)?,
                _ => {}
            }
        }
    }
}

/// A browser driven over the DevTools protocol, with one page attached.
pub struct Browser {
    child: Child,
    ws: Ws,
    next: u64,
    /// The page's session.
    page: String,
    /// Sessions of the page's frames from other sites (cookie dialogs often live in one).
    frames: Vec<String>,
}

/// Starts `exe` headless with its own profile and opens a blank page.
pub fn launch(exe: &Path, profile: &Path) -> Result<Browser, String> {
    let port_file = profile.join("DevToolsActivePort");
    let _ = std::fs::remove_file(&port_file);
    let _ = std::fs::create_dir_all(profile);
    let mut child = Command::new(exe)
        .args(["--headless=new", "--disable-gpu", "--no-first-run", "--no-default-browser-check", "--hide-scrollbars", "--mute-audio"])
        .args(["--remote-debugging-port=0", "--remote-debugging-address=127.0.0.1", "--window-size=1440,900"])
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg("about:blank")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot start the browser: {e}"))?;
    // the browser writes its port and path into the profile
    let start = Instant::now();
    let url = loop {
        if let Ok(text) = std::fs::read_to_string(&port_file) {
            let mut lines = text.lines();
            if let (Some(port), Some(path)) = (lines.next(), lines.next()) {
                if !port.trim().is_empty() && path.starts_with('/') {
                    break format!("ws://127.0.0.1:{}{}", port.trim(), path.trim());
                }
            }
        }
        if let Ok(Some(_)) = child.try_wait() {
            return Err("the browser stopped right after it started".into());
        }
        if start.elapsed() > Duration::from_secs(20) {
            kill_tree(&mut child);
            return Err("the browser did not open its DevTools port".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let ws = match Ws::connect(&url) {
        Ok(w) => w,
        Err(e) => {
            kill_tree(&mut child);
            return Err(e);
        }
    };
    let mut b = Browser { child, ws, next: 0, page: String::new(), frames: Vec::new() };
    let made = b.call("Target.createTarget", "{\"url\":\"about:blank\"}", None)?;
    let target = made.get("targetId").and_then(Json::as_str).ok_or("no page target")?.to_string();
    let attached = b.call("Target.attachToTarget", &format!("{{\"targetId\":{},\"flatten\":true}}", json_str(&target)), None)?;
    b.page = attached.get("sessionId").and_then(Json::as_str).ok_or("no page session")?.to_string();
    let page = b.page.clone();
    // a desktop browser's user agent (a headless one names itself, and some sites turn it away)
    let version = b.call("Browser.getVersion", "{}", None)?;
    let ua = version.get("userAgent").and_then(Json::as_str).unwrap_or("").replace("HeadlessChrome", "Chrome");
    if !ua.is_empty() {
        b.call(
            "Network.setUserAgentOverride",
            &format!("{{\"userAgent\":{},\"acceptLanguage\":\"en-US,en;q=0.9\"}}", json_str(&ua)),
            Some(&page),
        )?;
    }
    b.call("Page.enable", "{}", Some(&page))?;
    // the look the clone reads: a desktop window in light mode, with motion
    b.call(
        "Emulation.setEmulatedMedia",
        "{\"features\":[{\"name\":\"prefers-color-scheme\",\"value\":\"light\"},{\"name\":\"prefers-reduced-motion\",\"value\":\"no-preference\"}]}",
        Some(&page),
    )?;
    b.call("Target.setAutoAttach", "{\"autoAttach\":true,\"waitForDebuggerOnStart\":false,\"flatten\":true}", Some(&page))?;
    Ok(b)
}

impl Browser {
    /// Sends a command and waits for its result (events that arrive meanwhile are handled).
    pub fn call(&mut self, method: &str, params: &str, session: Option<&str>) -> Result<Json, String> {
        self.call_for(method, params, session, Duration::from_secs(30))
    }

    fn call_for(&mut self, method: &str, params: &str, session: Option<&str>, wait: Duration) -> Result<Json, String> {
        self.next += 1;
        let id = self.next;
        let sess = session.map(|s| format!(",\"sessionId\":{}", json_str(s))).unwrap_or_default();
        self.ws.send(&format!("{{\"id\":{id},\"method\":{},\"params\":{params}{sess}}}", json_str(method)))?;
        let deadline = Instant::now() + wait;
        loop {
            let Some(text) = self.ws.recv(deadline)? else {
                return Err(format!("{method}: no answer"));
            };
            let Ok(msg) = Json::parse(&text) else { continue };
            if msg.get("id").and_then(Json::as_f64) == Some(id as f64) {
                if let Some(e) = msg.get("error") {
                    let m = e.get("message").and_then(Json::as_str).unwrap_or("error");
                    return Err(format!("{method}: {m}"));
                }
                return Ok(msg.get("result").cloned().unwrap_or(Json::Null));
            }
            self.event(&msg);
        }
    }

    fn event(&mut self, msg: &Json) {
        let params = msg.get("params");
        match msg.get("method").and_then(Json::as_str) {
            Some("Target.attachedToTarget") => {
                let p = params.unwrap_or(&Json::Null);
                let kind = p.get("targetInfo").and_then(|t| t.get("type")).and_then(Json::as_str);
                if let (Some("iframe"), Some(s)) = (kind, p.get("sessionId").and_then(Json::as_str)) {
                    self.frames.push(s.to_string());
                }
            }
            Some("Target.detachedFromTarget") => {
                if let Some(s) = params.and_then(|p| p.get("sessionId")).and_then(Json::as_str) {
                    self.frames.retain(|f| f != s);
                }
            }
            _ => {}
        }
    }

    /// Evaluates a script in the page (or a frame) and returns its value as a string ("" when it
    /// has none or fails).
    pub fn eval_in(&mut self, session: &str, script: &str, wait: Duration) -> String {
        let params = format!("{{\"expression\":{},\"returnByValue\":true,\"awaitPromise\":true}}", json_str(script));
        match self.call_for("Runtime.evaluate", &params, Some(session), wait) {
            Ok(r) => {
                let v = r.get("result").and_then(|x| x.get("value"));
                match v {
                    Some(Json::Str(s)) => s.clone(),
                    Some(Json::Num(n)) => n.clone(),
                    Some(Json::Bool(b)) => b.to_string(),
                    _ => String::new(),
                }
            }
            Err(_) => String::new(),
        }
    }

    pub fn eval(&mut self, script: &str) -> String {
        let page = self.page.clone();
        self.eval_in(&page, script, Duration::from_secs(30))
    }

    pub fn navigate(&mut self, url: &str) -> Result<(), String> {
        let page = self.page.clone();
        let r = self.call("Page.navigate", &format!("{{\"url\":{}}}", json_str(url)), Some(&page))?;
        if let Some(e) = r.get("errorText").and_then(Json::as_str).filter(|e| !e.is_empty()) {
            return Err(format!("cannot open {url}: {e}"));
        }
        Ok(())
    }

    /// Waits until the document has loaded and no new resources arrived for `quiet`, at most
    /// `max`.
    pub fn settle(&mut self, quiet: Duration, max: Duration) {
        let start = Instant::now();
        let mut last = String::new();
        let mut since = Instant::now();
        while start.elapsed() < max {
            let state = self.eval(
                "performance.setResourceTimingBufferSize(100000), document.readyState + ' ' + performance.getEntriesByType('resource').length + ' ' + document.documentElement.outerHTML.length",
            );
            if state != last {
                last = state;
                since = Instant::now();
            } else if last.starts_with("complete") && since.elapsed() >= quiet {
                return;
            }
            // the events that arrived (frames attached) are read by the next call
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    /// Turns down a cookie or consent dialog in the page or one of its frames: clicks the most
    /// private choice ("Reject all", "Only necessary", "Odmítnout vše", "Alle ablehnen", ...).
    /// Returns the label it clicked, or "".
    pub fn reject_consent(&mut self) -> String {
        let mut sessions = vec![self.page.clone()];
        sessions.extend(self.frames.clone());
        for s in sessions {
            let got = self.eval_in(&s, CONSENT_JS, Duration::from_secs(10));
            if !got.is_empty() {
                return got;
            }
        }
        String::new()
    }

    pub fn close(mut self) {
        let _ = self.ws.send("{\"id\":999999,\"method\":\"Browser.close\",\"params\":{}}");
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        kill_tree(&mut self.child);
    }
}

pub fn kill_tree(child: &mut Child) {
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

/// Finds the button of a cookie or consent dialog that refuses everything optional and clicks it.
/// Only exact labels count (after case and spaces), and only inside a box that talks about
/// cookies, consent or privacy; a label that accepts is never clicked.
pub const CONSENT_JS: &str = r#"(() => {
  const REJECT = ['reject all', 'reject all cookies', 'reject', 'decline all', 'decline', 'deny all', 'deny', 'refuse all', 'refuse',
    'disagree', 'i disagree', 'do not agree', 'only necessary', 'necessary only', 'only essential', 'essential only',
    'only necessary cookies', 'necessary cookies only', 'use necessary cookies only', 'use only necessary cookies',
    'allow only necessary', 'allow necessary only', 'allow necessary cookies only', 'accept only necessary',
    'accept necessary only', 'accept only essential', 'accept only necessary cookies', 'strictly necessary only',
    'reject optional cookies', 'reject non-essential', 'reject non-essential cookies', 'reject additional cookies',
    'continue without accepting', 'continue without agreeing', 'continue without consent',
    'odmítnout vše', 'odmítnout všechny', 'odmítnout', 'odmítnout cookies', 'nesouhlasím', 'pouze nezbytné', 'jen nezbytné',
    'povolit pouze nezbytné', 'přijmout pouze nezbytné', 'pouze nezbytné cookies', 'pokračovat bez souhlasu',
    'odmietnuť všetko', 'odmietnuť', 'iba nevyhnutné', 'len nevyhnutné',
    'alle ablehnen', 'ablehnen', 'nur notwendige', 'nur notwendige cookies', 'nur erforderliche', 'nur erforderliche cookies',
    'nur essenzielle', 'nur essenzielle cookies', 'nur notwendige cookies akzeptieren', 'nur notwendige akzeptieren',
    'nicht einverstanden', 'weiter ohne einwilligung', 'optionale ablehnen',
    'tout refuser', 'refuser', 'refuser tout', 'continuer sans accepter', 'rechazar todo', 'rechazar todas', 'rechazar',
    'rifiuta tutto', 'rifiuta', 'odrzuć wszystkie', 'odrzuć', 'alles weigeren', 'weigeren', 'rejeitar tudo', 'rejeitar'];
  const ABOUT = /cookie|consent|gdpr|privacy|privacidad|datenschutz|souhlas|soukrom|súhlas|zgod|rgpd|confidentialit|tracking/i;
  const norm = (s) => (s || '').toLowerCase().replace(/\s+/g, ' ').trim().replace(/[.!…]+$/, '');
  const all = [];
  const walk = (root) => {
    for (const el of root.querySelectorAll('button, [role=button], a, input[type=submit], input[type=button]')) all.push(el);
    for (const el of root.querySelectorAll('*')) if (el.shadowRoot) walk(el.shadowRoot);
  };
  walk(document);
  const shown = (el) => {
    const r = el.getBoundingClientRect();
    const s = getComputedStyle(el);
    return r.width > 0 && r.height > 0 && s.visibility !== 'hidden' && s.display !== 'none';
  };
  const about = (el) => {
    for (let n = el, k = 0; n && k < 12; k++) {
      const t = (n.textContent || '') + ' ' + (n.id || '') + ' ' + (typeof n.className === 'string' ? n.className : '');
      if (ABOUT.test(t.slice(0, 20000))) return true;
      n = n.parentElement || (n.getRootNode && n.getRootNode().host) || null;
    }
    return ABOUT.test(location.hostname);
  };
  let best = null;
  let rank = 1e9;
  for (const el of all) {
    const label = norm(el.innerText || el.value || el.getAttribute('aria-label'));
    const at = REJECT.indexOf(label);
    if (at < 0 || at >= rank || !shown(el) || !about(el)) continue;
    best = el;
    rank = at;
  }
  if (!best) return '';
  const label = (best.innerText || best.value || best.getAttribute('aria-label') || '').trim();
  best.click();
  return label;
})()"#;

/// Before the page is scrolled: elements that wait to be revealed (transparent, or moved or
/// scaled away from their place) get `data-nyra-pre="opacity x y scale blur"`, and short numbers
/// (counters that may count up) `data-nyra-num`.
pub const MOTION_BEFORE_JS: &str = r#"(() => {
  const state = (el) => {
    const s = getComputedStyle(el);
    let x = 0, y = 0, k = 1;
    const m = (s.transform || '').match(/matrix(3d)?\(([^)]+)\)/);
    if (m) {
      const v = m[2].split(',').map(Number);
      x = m[1] ? v[12] : v[4];
      y = m[1] ? v[13] : v[5];
      k = Math.hypot(v[0], v[1]);
    }
    if (s.translate && s.translate !== 'none') {
      const p = s.translate.split(' ').map(parseFloat);
      x += p[0] || 0;
      y += p[1] || 0;
    }
    if (s.scale && s.scale !== 'none') k *= parseFloat(s.scale) || 1;
    return { o: parseFloat(s.opacity), v: s.visibility, d: s.display, x, y, k, blur: /blur\(/.test(s.filter) };
  };
  window.__nyraState = state;
  let n = 0;
  for (const el of document.body.querySelectorAll('*')) {
    if (/^(SCRIPT|STYLE|NOSCRIPT|TEMPLATE|svg|path|g|use)$/.test(el.tagName)) continue;
    const t = state(el);
    if (t.d === 'none') continue;
    const moved = Math.abs(t.x) > 6 || Math.abs(t.y) > 6 || Math.abs(t.k - 1) > 0.04;
    const seen = el.textContent.trim() || el.querySelector('img, picture, video');
    if (((t.o < 0.05 || t.v === 'hidden') && (moved || seen)) || (moved && t.o < 0.95)) {
      el.setAttribute('data-nyra-pre', [t.o.toFixed(2), Math.round(t.x), Math.round(t.y), t.k.toFixed(2), t.blur ? 1 : 0].join(' '));
      n++;
    }
    const text = el.textContent.trim();
    if (!el.children.length && text.length <= 12 && /\d/.test(text) && /^[\s\d.,+%$€£kKmM×x/-]+$/.test(text)) {
      el.setAttribute('data-nyra-num', text);
    }
  }
  return String(n);
})()"#;

/// After the page was scrolled: what was revealed (and how it came in), what counted up, and
/// every animation still running (`document.getAnimations()`), written as
/// `data-nyra-motion="anim:rise;loop:@name@ms"`; the keyframes of the named animations go into
/// `<meta name="nyra-keyframes" content="name\nbody">`.
pub const MOTION_AFTER_JS: &str = r#"(() => {
  const state = window.__nyraState;
  if (!state) return '';
  const add = (el, v) => {
    const old = el.getAttribute('data-nyra-motion');
    if (old && old.split(';').includes(v)) return;
    el.setAttribute('data-nyra-motion', old ? old + ';' + v : v);
  };
  let reveals = 0, counts = 0, anims = 0;
  for (const el of document.querySelectorAll('[data-nyra-pre]')) {
    const [o0, x0, y0, k0, b0] = el.getAttribute('data-nyra-pre').split(' ').map(Number);
    const t = state(el);
    if (t.o < 0.9 || t.v === 'hidden' || Math.abs(t.x) > 4 || Math.abs(t.y) > 4 || Math.abs(t.k - 1) > 0.03) continue;
    let w = 'fade';
    if (Math.abs(x0) > Math.abs(y0) && Math.abs(x0) > 6) w = x0 < 0 ? 'slide' : 'slide-left';
    else if (Math.abs(y0) > 6) w = 'rise';
    else if (Math.abs(k0 - 1) > 0.04) w = 'zoom';
    else if (b0) w = 'blur';
    add(el, 'anim:' + w);
    reveals++;
  }
  for (const el of document.querySelectorAll('[data-nyra-num]')) {
    if (el.textContent.trim() !== el.getAttribute('data-nyra-num')) {
      add(el, 'anim:count');
      counts++;
    }
  }
  const kebab = (k) => k.replace(/[A-Z]/g, (c) => '-' + c.toLowerCase());
  const bodies = {};
  for (const a of document.getAnimations()) {
    const el = a.effect && a.effect.target;
    if (!(el instanceof Element) || (typeof CSSTransition !== 'undefined' && a instanceof CSSTransition)) continue;
    const timing = a.effect.getComputedTiming();
    const name = (a.animationName || a.id || 'moving').replace(/[^A-Za-z0-9_-]/g, '');
    if (!name) continue;
    if (!bodies[name]) {
      bodies[name] = a.effect.getKeyframes().map((f) => {
        const decls = Object.keys(f).filter((k) => !['offset', 'computedOffset', 'easing', 'composite'].includes(k))
          .map((k) => kebab(k) + ': ' + f[k]).join('; ');
        return Math.round((f.computedOffset ?? f.offset ?? 0) * 100) + '% { ' + decls + ' }';
      }).join(' ');
    }
    add(el, (timing.iterations === Infinity ? 'loop' : 'anim') + ':@' + name + '@' + Math.round(Number(timing.duration) || 0));
    anims++;
  }
  for (const [k, body] of Object.entries(bodies)) {
    const m = document.createElement('meta');
    m.name = 'nyra-keyframes';
    m.content = k + '\n' + body;
    document.head.append(m);
  }
  return reveals + ' ' + counts + ' ' + anims;
})()"#;

/// What only a browser knows, written into the DOM for the reader (`read.rs`): elements that are
/// not shown get `data-nyra-hidden`, and a column of links down the left side of the window next
/// to the main content (an app's side navigation) gets `data-nyra="sidebar"`.
pub const HINTS_JS: &str = r#"(() => {
  let hidden = 0;
  for (const el of document.body.querySelectorAll('*')) {
    if (el.tagName === 'IMG') el.setAttribute('data-nyra-w', String(Math.round(el.getBoundingClientRect().width)));
    const s = getComputedStyle(el);
    if (s.display === 'none' || (s.visibility === 'hidden' && !el.querySelector('*'))) {
      el.setAttribute('data-nyra-hidden', '');
      hidden++;
    }
  }
  const vh = innerHeight;
  const vw = innerWidth;
  const ok = (el) => {
    const r = el.getBoundingClientRect();
    if (r.left > 24 || r.width < 140 || r.width > 400 || r.height < vh * 0.6 || el.hasAttribute('data-nyra-hidden')) return false;
    if (el.querySelectorAll('a[href]').length < 5) return false;
    const right = document.elementFromPoint(Math.min(vw - 2, r.right + (vw - r.right) / 2), vh * 0.6);
    return Boolean(right) && !el.contains(right) && right !== document.body && right !== document.documentElement;
  };
  let side = null;
  for (const el of document.body.querySelectorAll('*')) {
    if (!ok(el) || (el.parentElement && ok(el.parentElement))) continue;
    if (!side || el.querySelectorAll('a[href]').length > side.querySelectorAll('a[href]').length) side = el;
  }
  if (side) side.setAttribute('data-nyra', 'sidebar');
  // the page's own colors and font, as the browser computed them
  const solid = (c) => c && c !== 'transparent' && !/rgba\([^)]*,\s*0\)$/.test(c);
  const bs = getComputedStyle(document.body);
  const hs = getComputedStyle(document.documentElement);
  const bg = solid(bs.backgroundColor) ? bs.backgroundColor : solid(hs.backgroundColor) ? hs.backgroundColor : 'rgb(255, 255, 255)';
  // the background of each wide box (a band of the page), so that the reader does not guess it
  for (const el of document.body.querySelectorAll('*')) {
    const r = el.getBoundingClientRect();
    if (r.width < vw * 0.85 || r.height < 120 || el.hasAttribute('data-nyra-hidden')) continue;
    const c = getComputedStyle(el).backgroundColor;
    el.setAttribute('data-nyra-bg', solid(c) ? c : 'transparent');
  }
  // links drawn as buttons (a filled or outlined box around a short label)
  for (const a of document.body.querySelectorAll('a[href]')) {
    const s = getComputedStyle(a);
    const r = a.getBoundingClientRect();
    const boxed = (solid(s.backgroundColor) && s.backgroundColor !== bs.backgroundColor) || parseFloat(s.borderTopWidth) >= 1 && parseFloat(s.borderLeftWidth) >= 1;
    if (boxed && r.height >= 24 && r.height <= 80 && r.width <= 420 && a.textContent.trim()) a.setAttribute('data-nyra-btn', '');
  }
  document.body.setAttribute('data-nyra-bg', bg);
  document.body.setAttribute('data-nyra-ink', bs.color);
  // styles that scripts inserted rule by rule (CSS-in-JS): their <style> is empty in the DOM
  let css = '';
  // (only the page's own <style> elements: a sheet without one is the browser's, an ad blocker's)
  for (const sh of document.styleSheets) {
    const owner = sh.ownerNode;
    if (!owner || owner.tagName !== 'STYLE' || owner.textContent.trim()) continue;
    try {
      for (const r of sh.cssRules) css += r.cssText + '\n';
    } catch (e) {}
    if (css.length > 2000000) break;
  }
  if (css) {
    const s = document.createElement('style');
    s.setAttribute('data-nyra-cssom', '');
    s.textContent = css;
    document.head.append(s);
  }
  return hidden + (side ? ' sidebar' : '') + ' ' + css.length;
})()"#;

/// Scrolls down the page a screen at a time (lazy images and lists load when seen), at most
/// `steps` screens, then back to the top.
pub const SCROLL_JS: &str = r#"(async (steps) => {
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  let y = 0;
  for (let i = 0; i < steps; i++) {
    y += Math.round(innerHeight * 0.85);
    scrollTo(0, y);
    await wait(350);
    if (y + innerHeight >= document.documentElement.scrollHeight) break;
  }
  scrollTo(0, 0);
  await wait(300);
  return String(y);
})(STEPS)"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaped_text_from_the_browser() {
        let j = Json::parse(r#"{"result":{"result":{"type":"string","value":"x£51 ♡"}}}"#).unwrap();
        assert_eq!(j.get("result").and_then(|r| r.get("result")).and_then(|r| r.get("value")).and_then(Json::as_str), Some("x£51 ♡"));
    }
}
