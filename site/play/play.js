/* Nyra playground. Vanilla JS, no dependencies.
   The compiler runs as WebAssembly in two workers (worker.js): one checks as you type, the other
   runs programs and can be stopped and replaced at any time, so the page never freezes. */
import { EXAMPLES } from "./examples.js?v=play5";

const $ = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => [...r.querySelectorAll(s)];
const RM = matchMedia("(prefers-reduced-motion: reduce)").matches;
const esc = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
/* `code` in messages and hints */
const md = (s) => esc(s || "").replace(/`([^`]+)`/g, "<code>$1</code>");
const plural = (n, w) => `${n} ${w}${n === 1 ? "" : "s"}`;
const store = {
  get(k) { try { return localStorage.getItem(k); } catch (_) { return null; } },
  set(k, v) { try { localStorage.setItem(k, v); } catch (_) { /* private mode */ } },
};

/* ---------------- syntax highlighting (the highlighter of ../main.js, with v0.6's words) ---------------- */
const LANG = {
  nyra: {
    kw: /^(fn|let|var|if|else|while|for|in|return|ret|struct|enum|match|use|inout|break|continue|step|ex|true|false|none|arena)$/,
    ty: /^(int|float|bool|str|char)$/,
  },
  js: {
    kw: /^(function|return|let|const|var|if|else|for|while|do|break|continue|new|throw|try|catch|finally|typeof|of|in|class|switch|case|default|null|undefined|true|false|this)$/,
    ty: /^(Math|Number|String|Array|Map|Object|BigInt|JSON|console|process)$/,
  },
};
const TOK = /(\/\/[^\n]*)|("(?:[^"\\\n]|\\.)*"?)|('(?:[^'\\\n]|\\.)*'?)|(`(?:[^`\\]|\\.)*`?)|(\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)|([A-Za-z_$][\w$]*)|(\s+)|([\s\S])/g;
function hl(src, lang = "nyra") {
  const L = LANG[lang]; let m, out = "";
  const sp = (c, t) => c ? `<span class="${c}">${esc(t)}</span>` : esc(t);
  TOK.lastIndex = 0;
  while ((m = TOK.exec(src))) {
    if (m[1]) out += sp("c", m[1]);
    else if (m[2]) {
      if (lang === "nyra" && m[2].includes("{")) m[2].split(/(\{[^{}]*\})/).forEach((p) => { if (p) out += sp(/^\{[^{}]+\}$/.test(p) ? "si" : "s", p); });
      else out += sp("s", m[2]);
    }
    else if (m[3] || m[4]) out += sp("s", m[3] || m[4]);
    else if (m[5]) out += sp("n", m[5]);
    else if (m[6]) {
      const w = m[6], nx = src[TOK.lastIndex]; let c = "";
      if (L.kw.test(w)) c = "k";
      else if (nx === "(" && L.ty.test(w)) c = "b";
      else if (L.ty.test(w)) c = "t";
      else if (nx === "(") c = "f";
      else if (/^[A-Z]/.test(w)) c = "t";
      out += sp(c, w);
    }
    else if (m[7]) out += m[7];
    else out += sp("p", m[8]);
  }
  return out;
}

/* ---------------- the compiler in a worker ---------------- */
class Compiler {
  constructor() { this.seq = 0; this.start(); }
  start() {
    this.pending = new Map();
    this.w = new Worker(new URL("./worker.js?v=play5", import.meta.url), { type: "module" });
    this.ready = new Promise((res, rej) => { this._ok = res; this._no = rej; });
    this.ready.catch(() => {});
    this.w.onmessage = ({ data }) => {
      if ("ready" in data) { data.ready ? this._ok(data) : this._no(new Error(data.error)); return; }
      const p = this.pending.get(data.id);
      if (p) { this.pending.delete(data.id); clearTimeout(p.t); p.res(data); }
    };
    this.w.onerror = (e) => { this._no(new Error(e.message || "the worker failed to start")); };
  }
  /* -> {result} | {crash, stack?} | {timeout} */
  call(op, args, timeout = 0) {
    const id = ++this.seq;
    return new Promise((res) => {
      const p = { res, t: 0 };
      if (timeout) p.t = setTimeout(() => { this.pending.delete(id); this.restart(); res({ timeout: true }); }, timeout);
      this.pending.set(id, p);
      this.w.postMessage({ id, op, args });
    });
  }
  restart() {
    this.w.terminate();
    for (const p of this.pending.values()) { clearTimeout(p.t); p.res({ crash: "restarted" }); }
    this.start();
  }
}

/* ---------------- elements and state ---------------- */
const ta = $("#editor"), hlEl = $("#hl"), layer = $("#layer"), lines = $("#lines"), marks = $("#marks"), nums = $("#nums");
const probs = $("#probs"), status = $("#status"), out = $("#out"), meta = $("#meta"), stdin = $("#stdin");
const runBtn = $("#runBtn"), fixBtn = $("#fixBtn"), fmtBtn = $("#fmtBtn"), shareBtn = $("#shareBtn"), fixN = $("#fixN");
const S = {
  problems: [],      // errors and warnings of the last check or run: {code, message, line, col, hint, fix, warning}
  runtime: null,     // the runtime error of the last run, until the next edit
  fixedInMemory: 0,  // fixes the last run applied in memory
  example: null,
  running: false,
  edited: false,
  js: { src: null, text: "" },
  explained: new Map(),
};
let checker, runner;

/* ---------------- rendering the editor ---------------- */
const LH = () => parseFloat(getComputedStyle(ta).lineHeight) || 22;
const PAD = () => parseFloat(getComputedStyle(ta).paddingTop) || 14;
function visualCol(text, col) {
  /* columns count characters; a tab moves to the next multiple of 4 */
  let v = 0; const cs = Array.from(text);
  for (let i = 0; i < col - 1 && i < cs.length; i++) v = cs[i] === "\t" ? (Math.floor(v / 4) + 1) * 4 : v + 1;
  return v;
}
function tokenLen(text, col) {
  const rest = Array.from(text).slice(col - 1).join("");
  const m = rest.match(/^[A-Za-z_]\w*|^\d+(\.\d+)?|^"[^"]*"?|^'[^']*'?|^\S/);
  return m ? Array.from(m[0]).length : 1;
}
function render() {
  const src = ta.value;
  hlEl.innerHTML = hl(src) + "\n";
  const n = src.split("\n").length;
  if (nums.childElementCount !== n) nums.innerHTML = Array.from({ length: n }, (_, i) => `<div>${i + 1}</div>`).join("");
  paintMarks();
}
function paintMarks(flash) {
  const srcLines = ta.value.split("\n"), lh = LH(), pad = PAD();
  let bars = "", sq = "";
  const bad = new Map();
  const all = S.problems.slice();
  if (S.runtime) all.push({ ...S.runtime, runtime: true });
  for (const p of all) {
    if (!p.line || p.line > srcLines.length) continue;
    const kind = p.warning ? "warn" : "err";
    if (bad.get(p.line) !== "err") bad.set(p.line, kind);
    const text = srcLines[p.line - 1];
    const v = visualCol(text, p.col || 1), w = Math.max(1, tokenLen(text, p.col || 1));
    sq += `<i class="squig ${kind}" style="top:${pad + p.line * lh - 4}px;left:calc(var(--padx) + ${v}ch);width:${w}ch"></i>`;
  }
  for (const [ln, kind] of bad) bars += `<i class="lb ${kind}" style="top:${pad + (ln - 1) * lh}px"></i>`;
  if (flash) for (const ln of flash) bars += `<i class="lb flash" style="top:${pad + (ln - 1) * lh}px"></i>`;
  lines.innerHTML = bars;
  marks.innerHTML = sq;
  $$("div", nums).forEach((d, i) => { d.className = bad.get(i + 1) || ""; });
}
function syncScroll() {
  layer.style.transform = `translate3d(${-ta.scrollLeft}px, ${-ta.scrollTop}px, 0)`;
  nums.style.transform = `translate3d(0, ${-ta.scrollTop}px, 0)`;
}
ta.addEventListener("scroll", syncScroll, { passive: true });

/* ---------------- editing ---------------- */
function insertText(text) {
  /* execCommand keeps the browser's undo history; setRangeText is the fallback */
  if (!document.execCommand || !document.execCommand("insertText", false, text)) {
    ta.setRangeText(text, ta.selectionStart, ta.selectionEnd, "end");
    ta.dispatchEvent(new Event("input"));
  }
}
function replaceAll(text) {
  if (text === ta.value) return;
  const before = ta.value.split("\n");
  const at = ta.selectionStart, top = ta.scrollTop;
  ta.focus({ preventScroll: true });
  ta.select();
  insertText(text);
  const pos = Math.min(at, text.length);
  ta.setSelectionRange(pos, pos);
  ta.scrollTop = top;
  /* the lines that changed glow for a moment */
  const after = text.split("\n");
  let a = 0; while (a < before.length && a < after.length && before[a] === after[a]) a++;
  let b = 0; while (b < before.length - a && b < after.length - a && before[before.length - 1 - b] === after[after.length - 1 - b]) b++;
  const changed = []; for (let i = a; i < after.length - b; i++) changed.push(i + 1);
  if (changed.length && !RM) { paintMarks(changed); setTimeout(() => paintMarks(), 1300); }
}
function lineStart(pos) { return ta.value.lastIndexOf("\n", pos - 1) + 1; }
ta.addEventListener("keydown", (e) => {
  const v = ta.value, s = ta.selectionStart, en = ta.selectionEnd;
  if ((e.ctrlKey || e.metaKey) && e.key === "Enter") { e.preventDefault(); run(); return; }
  if (e.shiftKey && e.altKey && (e.key === "F" || e.key === "f" || e.code === "KeyF")) { e.preventDefault(); format(); return; }
  if (e.key === "Escape") { ta.dataset.esc = "1"; return; }
  if (e.key === "Tab" && !e.ctrlKey && !e.altKey && !e.metaKey) {
    if (ta.dataset.esc) { delete ta.dataset.esc; return; } /* Escape, then Tab: leave the editor */
    e.preventDefault();
    const ls = lineStart(s);
    if (s === en && !e.shiftKey) { insertText("    ".slice((s - ls) % 4)); return; }
    /* indent or outdent every selected line */
    const le = v.indexOf("\n", en - (en > s && v[en - 1] === "\n" ? 1 : 0)); const end = le < 0 ? v.length : le;
    const block = v.slice(ls, end).split("\n");
    const next = block.map((l) => e.shiftKey ? l.replace(/^( {1,4}|\t)/, "") : "    " + l).join("\n");
    ta.setSelectionRange(ls, end); insertText(next);
    ta.setSelectionRange(ls, ls + next.length);
    return;
  }
  delete ta.dataset.esc;
  if (e.key === "Enter" && !e.shiftKey && !e.ctrlKey && !e.metaKey && !e.altKey && s === en) {
    e.preventDefault();
    const line = v.slice(lineStart(s), s);
    let ind = line.match(/^\s*/)[0];
    const opens = /[{([]\s*$/.test(line), closes = /^\s*[})\]]/.test(v.slice(s, v.indexOf("\n", s) < 0 ? v.length : v.indexOf("\n", s)));
    if (opens) {
      if (closes) { insertText("\n" + ind + "    " + "\n" + ind); ta.setSelectionRange(s + 1 + ind.length + 4, s + 1 + ind.length + 4); return; }
      ind += "    ";
    }
    insertText("\n" + ind);
    return;
  }
  if ((e.key === "}" || e.key === ")" || e.key === "]") && s === en) {
    const ls = lineStart(s), before = v.slice(ls, s);
    if (/^ {4,}$/.test(before)) { e.preventDefault(); ta.setSelectionRange(s - 4, s); insertText(e.key); }
  }
});
let checkT = 0, saveT = 0;
ta.addEventListener("input", () => {
  S.runtime = null;
  render();
  setStatus("busy", "checking");
  clearTimeout(checkT); checkT = setTimeout(check, 260);
  if (!S.edited) { S.edited = true; if (location.hash) history.replaceState(null, "", location.pathname + location.search); }
  clearTimeout(saveT); saveT = setTimeout(() => store.set("nyra-play-src", ta.value), 400);
  if (S.example) { S.example = null; paintChips(); }
});
stdin.addEventListener("input", () => { clearTimeout(saveT); saveT = setTimeout(() => store.set("nyra-play-stdin", stdin.value), 400); });

/* ---------------- problems ---------------- */
function setStatus(kind, text) {
  status.className = "badge" + (kind === "ok" ? " ok" : kind === "err" ? " err" : kind === "busy" ? " busy" : "");
  status.textContent = text;
}
function setProblems(errors = [], warnings = []) {
  S.problems = [...errors.map((e) => ({ ...e })), ...warnings.map((w) => ({ ...w, warning: true }))];
  const ne = errors.length, nw = warnings.length;
  setStatus(ne ? "err" : "ok", ne ? plural(ne, "error") : nw ? plural(nw, "warning") : "no errors");
  paintMarks();
  paintProblems();
  updateFix();
}
function updateFix() {
  const n = Math.max(S.problems.filter((p) => p.fix && !p.warning).length, S.fixedInMemory);
  fixBtn.disabled = n === 0;
  fixN.hidden = n === 0; fixN.textContent = n;
}
/* the source with a fix's edits applied (columns count characters; the end is exclusive) */
function applyEdits(src, edits) {
  const ls = src.split("\n").map((l) => Array.from(l));
  const off = (line, col) => { let o = 0; for (let i = 0; i < line - 1; i++) o += ls[i].length + 1; return o + col - 1; };
  const chars = Array.from(src);
  const sorted = edits.map((e) => [off(e.line, e.col), off(e.end_line, e.end_col), e.text]).sort((a, b) => b[0] - a[0]);
  for (const [a, b, t] of sorted) chars.splice(a, b - a, ...Array.from(t));
  return chars.join("");
}
function fixPreview(p) {
  try {
    const fixed = applyEdits(ta.value, p.fix).split("\n");
    const first = Math.min(...p.fix.map((e) => e.line));
    const line = (fixed[first - 1] || "").trim();
    return line ? `\`${line}\`` : "delete the line";
  } catch (_) { return ""; }
}
function paintProblems() {
  const list = S.problems.slice();
  if (S.runtime) list.unshift({ ...S.runtime, runtime: true });
  probs.classList.toggle("on", list.length > 0);
  probs.innerHTML = list.map((p, i) => `
    <div class="pb ${p.warning ? "warn" : "err"}" style="--k:${Math.min(i, 8)}">
      <div class="pb-h">
        <button type="button" class="pb-at" data-line="${p.line}" data-col="${p.col}" title="Go to line ${p.line}">${p.line}:${p.col}</button>
        <b class="pb-code">${esc(p.code)}</b>
        <span class="pb-kind">${p.runtime ? "runtime error" : p.warning ? "warning" : "error"}</span>
      </div>
      <p class="pb-msg">${md(p.message)}</p>
      ${p.hint ? `<p class="pb-hint"><span>hint</span> ${md(p.hint)}</p>` : ""}
      <div class="pb-acts">
        ${p.fix && !p.warning ? `<button type="button" class="pb-fix" data-i="${i}">Apply fix<span>${md(fixPreview(p))}</span></button>` : ""}
        <button type="button" class="pb-exp" data-code="${esc(p.code)}" aria-expanded="false">nyra explain ${esc(p.code)}</button>
      </div>
      <pre class="pb-ex" hidden></pre>
    </div>`).join("");
  probs._list = list;
}
probs.addEventListener("click", async (e) => {
  const at = e.target.closest(".pb-at"), fx = e.target.closest(".pb-fix"), ex = e.target.closest(".pb-exp");
  if (at) goTo(+at.dataset.line, +at.dataset.col);
  if (fx) { const p = probs._list[+fx.dataset.i]; if (p && p.fix) replaceAll(applyEdits(ta.value, p.fix)); }
  if (ex) {
    const pre = ex.closest(".pb").querySelector(".pb-ex"), open = ex.getAttribute("aria-expanded") === "true";
    ex.setAttribute("aria-expanded", String(!open));
    if (open) { pre.hidden = true; return; }
    pre.hidden = false;
    pre.textContent = S.explained.get(ex.dataset.code) || "…";
    if (!S.explained.has(ex.dataset.code)) {
      const r = await checker.call("explain", [ex.dataset.code]);
      const text = r.result && r.result.text ? r.result.text : (r.result && r.result.error) || "no entry for this code";
      S.explained.set(ex.dataset.code, text);
      pre.textContent = text;
    }
  }
});
function goTo(line, col) {
  const ls = ta.value.split("\n");
  let pos = 0; for (let i = 0; i < line - 1 && i < ls.length; i++) pos += ls[i].length + 1;
  const l = ls[line - 1] || "";
  pos += Array.from(l).slice(0, Math.max(0, col - 1)).join("").length;
  ta.focus({ preventScroll: true });
  ta.setSelectionRange(pos, pos);
  const lh = LH(), y = PAD() + (line - 1) * lh;
  if (y < ta.scrollTop || y > ta.scrollTop + ta.clientHeight - lh * 2) ta.scrollTop = Math.max(0, y - ta.clientHeight / 3);
  $("#edWin").scrollIntoView({ block: "nearest", behavior: RM ? "auto" : "smooth" });
}

/* ---------------- check, run, fix, format ---------------- */
let checkSeq = 0;
async function check() {
  const seq = ++checkSeq, src = ta.value;
  const r = await checker.call("check", [src], 20000);
  if (seq !== checkSeq) return;
  if (r.result) setProblems(r.result.errors, r.result.warnings || []);
  else if (r.crash) { setStatus("err", "compiler error"); toast(r.stack ? "The checker ran out of stack on this program." : "The compiler hit an internal error: " + r.crash); }
  if (!$("#panJs").hidden) showJs();
}

const LIMIT_NOTE = { E0355: "the step limit", E0356: "the memory limit", E0357: "the output limit", E0358: "the depth limit", E0359: "the time limit" };
async function run() {
  if (S.running) return;
  S.running = true;
  runBtn.classList.add("busy"); runBtn.setAttribute("aria-busy", "true");
  selectTab("out");
  const src = ta.value, t0 = performance.now();
  const lead = `<span class="cmd">$ nyra run main.nyra --sandbox --allow input</span>\n`;
  out.classList.add("dim-all");
  const r = await runner.call("run", [src, stdin.value, ""], 12000);
  const ms = performance.now() - t0;
  S.running = false;
  runBtn.classList.remove("busy"); runBtn.removeAttribute("aria-busy");
  out.classList.remove("dim-all");
  S.runtime = null; S.fixedInMemory = 0;
  let body = "", tail = "", exit = null;
  if (r.timeout) {
    body = `<span class="o-err">stopped: the run took longer than 12 seconds, so the worker was replaced</span>\n`;
    meta.textContent = "stopped";
  } else if (r.crash) {
    if (r.stack) {
      body = `<span class="o-err">runtime error[E0358]: the recursion went deeper than this browser's stack allows</span>\n<span class="o-hint">  = hint: recurse less deeply, or loop instead; natively the limit is 20,000 nested calls</span>\n`;
      exit = 123;
      meta.textContent = "exit 123 · stack";
    } else {
      body = `<span class="o-err">internal error in the compiler: ${esc(r.crash)}</span>\n`;
      meta.textContent = "internal error";
    }
  } else {
    const res = r.result; exit = res.exit;
    const stdout = res.stdout || "";
    /* the first lines slide in one by one; a long output is shown up to VIEW characters as plain text */
    const VIEW = 100000, ANIM = 60;
    const shownOut = stdout.length > VIEW ? stdout.slice(0, stdout.lastIndexOf("\n", VIEW) + 1 || VIEW) : stdout;
    const outLines = shownOut.split("\n");
    if (outLines[outLines.length - 1] === "") outLines.pop();
    body = outLines.slice(0, ANIM).map((l, i) => `<span class="ln" style="--k:${i}">${esc(l)}</span>\n`).join("") +
      (outLines.length > ANIM ? esc(outLines.slice(ANIM).join("\n") + "\n") : "");
    if (shownOut.length < stdout.length) {
      const rest = stdout.length - shownOut.length;
      body += `<span class="dim">… ${rest.toLocaleString("en")} more characters of output are not shown here</span>\n`;
    }
    if (res.stage === "compile") {
      setProblems(res.errors || [], res.warnings || []);
      tail += res.errors.map((e) => `<span class="o-err">error[${e.code}]</span>: ${md(e.message)}\n<span class="o-at">  --> main.nyra:${e.line}:${e.col}</span>\n`).join("");
      tail += `<span class="dim">nyra: ${plural(res.errors.length, "error")}; nothing ran</span>\n`;
    } else {
      if (res.fixed && res.fixed.length) {
        S.fixedInMemory = res.fixed.length;
        updateFix();
        const shown = res.fixed.slice(0, 3).map((f) => `<span class="o-fix">  fixed[${f.code}]</span> <span class="dim">→</span> ${md(f.applied || "")}\n`).join("");
        const more = res.fixed.length > 3 ? `<span class="dim">  and ${res.fixed.length - 3} more</span>\n` : "";
        body = `<span class="o-fix">nyra: ${plural(res.fixed.length, "error")} with a certain fix, repaired in memory as <code>nyra run</code> does:</span>\n` + shown + more +
          `<span class="dim">  press Fix to write ${res.fixed.length === 1 ? "it" : "them"} into your code</span>\n\n` + body;
      }
      if (res.errors && res.errors.length) {
        const e = res.errors[0];
        S.runtime = { code: e.code, message: e.message, line: e.line, col: e.col, hint: e.hint };
        tail += `<span class="o-err">runtime error[${e.code}]: ${md(e.message)}</span>\n<span class="o-at">  --> main.nyra:${e.line}:${e.col}</span>\n<span class="o-hint">  = hint: ${md(e.hint)}</span>\n`;
        if (LIMIT_NOTE[e.code]) tail += `<span class="dim">  stopped by ${LIMIT_NOTE[e.code]} of the playground</span>\n`;
        paintMarks(); paintProblems();
      }
      if (res.stderr) tail += `<span class="o-err">${esc(res.stderr)}</span>\n`;
      if (!stdout && !tail && !res.fixed) body = `<span class="dim">(the program printed nothing)</span>\n`;
    }
    const steps = res.steps != null ? ` · ${res.steps.toLocaleString("en")} steps` : "";
    meta.textContent = res.stage === "compile" ? "compile errors" : `exit ${res.exit} · ${(res.ms ? res.ms.run : ms).toFixed(1)} ms${steps}`;
  }
  if (exit != null) tail += `<span class="ex ${exit === 0 ? "ok" : "bad"}">exit ${exit}</span>`;
  out.innerHTML = lead + body + tail;
  out.classList.remove("show"); void out.offsetWidth; out.classList.add("show");
  out.scrollTop = 0;
}

async function fixAll() {
  const r = await checker.call("fix", [ta.value], 20000);
  if (!r.result) { toast("The fixer failed on this program."); return; }
  const res = r.result, n = res.fixed.length;
  if (!n) { toast(res.errors.length ? "No error left has a certain fix: the hints say what to change." : "Nothing to fix."); return; }
  replaceAll(res.source);
  S.fixedInMemory = 0;
  toast(`Fixed ${plural(n, "error")}` + (res.errors.length ? `; ${res.errors.length} left without a certain fix` : ""));
}
async function format() {
  const r = await checker.call("fmt", [ta.value], 20000);
  if (!r.result) { toast("The formatter failed on this program."); return; }
  if (!r.result.ok) { toast("Format needs a program that compiles: fix the errors first."); if (r.result.errors) setProblems(r.result.errors, r.result.warnings || []); return; }
  if (r.result.source === ta.value) { toast("Already formatted."); return; }
  replaceAll(r.result.source);
  toast(r.result.fixed && r.result.fixed.length ? `Formatted, with ${plural(r.result.fixed.length, "fix")}` : "Formatted");
}
runBtn.addEventListener("click", run);
fixBtn.addEventListener("click", fixAll);
fmtBtn.addEventListener("click", format);
addEventListener("keydown", (e) => { if ((e.ctrlKey || e.metaKey) && e.key === "Enter" && e.target !== ta) { e.preventDefault(); run(); } });

/* ---------------- output tabs and the JavaScript view ---------------- */
const tabs = [$("#tabOut"), $("#tabJs")], pans = [$("#panOut"), $("#panJs")];
function selectTab(which) {
  const i = which === "js" ? 1 : 0;
  tabs.forEach((t, k) => { t.setAttribute("aria-selected", String(k === i)); t.tabIndex = k === i ? 0 : -1; });
  pans.forEach((p, k) => { p.hidden = k !== i; });
  if (i === 1) showJs();
}
tabs[0].addEventListener("click", () => selectTab("out"));
tabs[1].addEventListener("click", () => selectTab("js"));
tabs.forEach((t, k) => t.addEventListener("keydown", (e) => {
  if (e.key === "ArrowRight" || e.key === "ArrowLeft") { e.preventDefault(); const j = 1 - k; selectTab(j ? "js" : "out"); tabs[j].focus(); }
}));
async function showJs() {
  const src = ta.value, el = $("#jsOut");
  if (S.js.src === src) return;
  const r = await checker.call("js", [src], 20000);
  if (ta.value !== src) return;
  S.js = { src, text: "" };
  if (r.result && r.result.ok) el.innerHTML = hl(r.result.js, "js");
  else el.innerHTML = `<span class="dim">// the program has errors: the JavaScript appears when it compiles</span>`;
}

/* ---------------- examples ---------------- */
const chips = $("#chips"), note = $("#exNote");
chips.innerHTML = EXAMPLES.map((e) => `<button type="button" data-id="${e.id}" aria-pressed="false">${esc(e.label)}</button>`).join("");
function paintChips() {
  $$("button", chips).forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.id === S.example)));
  const ex = EXAMPLES.find((e) => e.id === S.example);
  note.innerHTML = ex ? md(ex.note) : "Your program. Pick an example to start from another one.";
}
function loadExample(id, fromHash) {
  const ex = EXAMPLES.find((e) => e.id === id) || EXAMPLES[0];
  ta.value = ex.code;
  stdin.value = ex.stdin || "";
  S.example = ex.id; S.edited = false; S.runtime = null; S.fixedInMemory = 0;
  ta.scrollTop = 0; ta.scrollLeft = 0; syncScroll();
  render(); paintChips();
  if (!fromHash) history.replaceState(null, "", "#ex=" + ex.id);
  store.set("nyra-play-src", ""); store.set("nyra-play-stdin", "");
  $(".ed").classList.remove("swap"); void ta.offsetWidth; $(".ed").classList.add("swap");
  out.innerHTML = `<span class="cmd">$ nyra run main.nyra --sandbox --allow input</span>\n<span class="dim">Press Run, or Ctrl+Enter.</span>`;
  meta.textContent = "";
  if (checker) { setStatus("busy", "checking"); check(); }
}
chips.addEventListener("click", (e) => {
  const b = e.target.closest("button");
  if (b) { loadExample(b.dataset.id); b.scrollIntoView({ block: "nearest", inline: "nearest", behavior: RM ? "auto" : "smooth" }); }
});

/* ---------------- share: the program in the URL hash ---------------- */
const b64u = (bytes) => { let s = ""; for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000)); return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, ""); };
const unb64u = (s) => Uint8Array.from(atob(s.replace(/-/g, "+").replace(/_/g, "/")), (c) => c.charCodeAt(0));
async function pipe(bytes, stream) { return new Uint8Array(await new Response(new Blob([bytes]).stream().pipeThrough(stream)).arrayBuffer()); }
async function encode(text) {
  const raw = new TextEncoder().encode(text);
  if (typeof CompressionStream === "function") {
    try { return "z=" + b64u(await pipe(raw, new CompressionStream("deflate-raw"))); } catch (_) { /* plain below */ }
  }
  return "code=" + b64u(raw);
}
async function decode(kind, data) {
  let bytes = unb64u(data);
  if (kind === "z") bytes = await pipe(bytes, new DecompressionStream("deflate-raw"));
  return new TextDecoder().decode(bytes);
}
async function share() {
  let hash = "#" + await encode(ta.value);
  if (stdin.value) hash += "&in=" + b64u(new TextEncoder().encode(stdin.value));
  history.replaceState(null, "", hash);
  S.edited = false;
  const url = location.href;
  try { await navigator.clipboard.writeText(url); toast("Link copied: it holds the whole program"); }
  catch (_) { toast("Link ready in the address bar"); }
}
shareBtn.addEventListener("click", share);
async function fromHash() {
  const h = location.hash.slice(1);
  if (!h) return false;
  const q = new URLSearchParams(h);
  try {
    if (q.has("ex")) { loadExample(q.get("ex"), true); return true; }
    const kind = q.has("z") ? "z" : q.has("code") ? "code" : null;
    if (!kind) return false;
    ta.value = await decode(kind, q.get(kind));
    stdin.value = q.has("in") ? new TextDecoder().decode(unb64u(q.get("in"))) : "";
    S.example = null; render(); paintChips();
    return true;
  } catch (_) { toast("That link's program could not be read."); return false; }
}
addEventListener("hashchange", async () => { if (await fromHash()) { setStatus("busy", "checking"); check(); } });

/* ---------------- small things: toast, nav, reveals ---------------- */
let toastT = 0;
function toast(msg) {
  const t = $("#toast");
  t.textContent = msg; t.classList.add("on");
  clearTimeout(toastT); toastT = setTimeout(() => t.classList.remove("on"), 2600);
}
function nav() {
  const bar = $("#topbar"), btn = $(".nav-menu"), sheet = $("#msheet");
  let open = false;
  const set = (v) => {
    if (v === open) return; open = v;
    btn.setAttribute("aria-expanded", String(v)); $(".sr", btn).textContent = v ? "Close menu" : "Menu";
    document.documentElement.classList.toggle("menu-open", v);
    if (v) { sheet.hidden = false; void sheet.offsetWidth; sheet.classList.add("shown"); }
    else { sheet.classList.remove("shown"); setTimeout(() => { if (!open) sheet.hidden = true; }, RM ? 0 : 420); btn.focus(); }
  };
  btn.addEventListener("click", () => set(!open));
  addEventListener("keydown", (e) => { if (open && e.key === "Escape") set(false); });
  addEventListener("scroll", () => bar.classList.toggle("scrolled", scrollY > 24), { passive: true });
}
function reveals() {
  if (RM || !("IntersectionObserver" in window)) { $$(".rv").forEach((e) => e.classList.add("in")); return; }
  const ro = new IntersectionObserver((es) => es.forEach((e) => { if (e.isIntersecting) { e.target.classList.add("in"); ro.unobserve(e.target); } }), { rootMargin: "0px 0px -5% 0px", threshold: 0.05 });
  $$(".rv").forEach((e) => ro.observe(e));
}

/* ---------------- start ---------------- */
async function main() {
  nav(); reveals();
  if (!(await fromHash())) {
    const draft = store.get("nyra-play-src");
    if (draft) { ta.value = draft; stdin.value = store.get("nyra-play-stdin") || ""; S.example = null; render(); paintChips(); }
    else loadExample("tour", true);
  }
  syncScroll();
  addEventListener("resize", () => { paintMarks(); syncScroll(); });
  setStatus("busy", "loading");
  runBtn.disabled = true;
  try {
    checker = new Compiler(); runner = new Compiler();
    const info = await checker.ready;
    $("#ver").textContent = `v${info.version} · WebAssembly · loaded in ${info.ms} ms`;
    runBtn.disabled = false;
    fetch("nyra.wasm?v=play5", { method: "HEAD" }).then((r) => {
      const n = +r.headers.get("content-length");
      if (n) $("#wasmSize").textContent = `${(n / 1048576).toFixed(2)} MB`;
    }).catch(() => {});
    check();
  } catch (e) {
    setStatus("err", "no compiler");
    $("#ver").textContent = "the compiler did not load";
    out.innerHTML = `<span class="o-err">The compiler could not start in this browser: ${esc(String(e.message || e))}</span>\n<span class="dim">The playground needs WebAssembly and module workers (any current browser).</span>`;
  }
}
main();
