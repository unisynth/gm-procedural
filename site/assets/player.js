// One audio player for the listening pages: four banks of the same material, switched at the same position.
// Keys: 1–4 bank, Space play/pause, ← → seek 5 s, L loop. Audio comes from the public bucket (CORS enabled, so the
// live spectrum can read it).
export const BANKS = [
  { k: "o", file: "orig", name: "gm.dls", sub: "original", c: "var(--b-o)" },
  { k: "s", file: "sf", name: "gmplus", sub: "original", c: "var(--b-s)" },
  { k: "g", file: "gen", name: "gen-gm", sub: "procedural", c: "var(--b-g)" },
  { k: "p", file: "plus", name: "gen-plus", sub: "procedural", c: "var(--b-p)" },
];
const fmt = (t) => (Number.isFinite(t) ? `${Math.floor(t / 60)}:${String(Math.floor(t % 60)).padStart(2, "0")}` : "0:00");

export class Deck {
  constructor(root, { onBank, onEnded } = {}) {
    this.root = root; this.onBank = onBank; this.onEnded = onEnded;
    this.bank = localStorage.getItem("gmp.bank") || "g";
    this.urls = null; this.loopOn = false;
    this.audio = new Audio(); this.audio.crossOrigin = "anonymous"; this.audio.preload = "auto";
    root.querySelector(".banks").innerHTML = BANKS.map((b, i) =>
      `<button data-k="${b.k}" style="--c:${b.c}" aria-pressed="false" title="key ${i + 1}">${b.name}<small>${i + 1} · ${b.sub}</small></button>`).join("");
    this.btns = [...root.querySelectorAll(".banks button")];
    this.btns.forEach((b) => b.addEventListener("click", () => this.setBank(b.dataset.k)));
    this.playBtn = root.querySelector(".play"); this.playBtn.addEventListener("click", () => this.toggle());
    this.timeEl = root.querySelector(".time");
    this.seekEl = root.querySelector(".seek"); this.seekCv = this.seekEl.querySelector("canvas");
    this.loopBtn = root.querySelector(".loop");
    this.loopBtn?.addEventListener("click", () => { this.loopOn = !this.loopOn; this.audio.loop = this.loopOn; this.loopBtn.setAttribute("aria-pressed", this.loopOn); });
    const seekTo = (e) => { const r = this.seekEl.getBoundingClientRect(); const d = this.audio.duration; if (d) this.audio.currentTime = Math.max(0, Math.min(d, ((e.clientX - r.left) / r.width) * d)); };
    let drag = false;
    this.seekEl.addEventListener("pointerdown", (e) => { drag = true; this.seekEl.setPointerCapture(e.pointerId); seekTo(e); });
    this.seekEl.addEventListener("pointermove", (e) => drag && seekTo(e));
    this.seekEl.addEventListener("pointerup", () => (drag = false));
    for (const ev of ["play", "pause"]) this.audio.addEventListener(ev, () => { this.playBtn.textContent = this.audio.paused ? "▶" : "❚❚"; this.playBtn.setAttribute("aria-label", this.audio.paused ? "Play" : "Pause"); });
    this.audio.addEventListener("ended", () => this.onEnded?.());
    // CORS is only needed for the spectrum. If a CORS load fails (and no graph exists yet), play without it.
    this.cors = true;
    this.audio.addEventListener("error", () => {
      if (!this.cors || this.ctx || !this.urls) return;
      this.cors = false; this.audio.removeAttribute("crossorigin"); this.swap(this.at ?? 0, this.wantPlay);
    });
    addEventListener("keydown", (e) => {
      if (e.target.closest("input, select, textarea") || e.metaKey || e.ctrlKey || e.altKey) return;
      if (e.key >= "1" && e.key <= "4") { this.setBank(BANKS[+e.key - 1].k); e.preventDefault(); }
      else if (e.key === " ") { this.toggle(); e.preventDefault(); }
      else if (e.key === "ArrowLeft") { this.audio.currentTime = Math.max(0, this.audio.currentTime - 5); e.preventDefault(); }
      else if (e.key === "ArrowRight") { this.audio.currentTime = Math.min(this.audio.duration || 0, this.audio.currentTime + 5); e.preventDefault(); }
      else if (e.key === "l" || e.key === "L") this.loopBtn?.click();
    });
    this.viz = root.querySelector(".viz");
    this.paint();
    this.mark();
  }
  // urls: { o, s, g, p } (missing = disabled)
  load(urls, { title, by, autoplay = true, at = 0 } = {}) {
    this.urls = urls;
    this.root.querySelector(".now .t").textContent = title;
    this.root.querySelector(".now .b").textContent = by || "";
    if (!urls[this.bank]) this.bank = BANKS.find((b) => urls[b.k]).k;
    this.btns.forEach((b) => (b.disabled = !urls[b.dataset.k]));
    this.mark();
    this.swap(at, autoplay);
  }
  setBank(k) {
    if (!this.urls?.[k] || k === this.bank) { this.bank = this.urls?.[k] ? k : this.bank; return; }
    this.bank = k; localStorage.setItem("gmp.bank", k); this.mark();
    this.swap(this.audio.currentTime, !this.audio.paused);
    this.onBank?.(k);
  }
  swap(at, play) {
    const a = this.audio; this.at = at; this.wantPlay = play;
    a.src = this.urls[this.bank];
    const go = () => { try { a.currentTime = Math.min(at, (a.duration || at + 1) - 0.05); } catch {} if (play) this.start(); };
    a.addEventListener("loadedmetadata", go, { once: true });
    a.load();
  }
  start() { this.ensureGraph(); this.ctx?.resume(); this.audio.play().catch(() => {}); }
  toggle() { if (!this.urls) return; this.audio.paused ? this.start() : this.audio.pause(); }
  mark() { this.btns.forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.k === this.bank))); this.root.style.setProperty("--cur", BANKS.find((b) => b.k === this.bank).c); }
  ensureGraph() {
    if (this.ctx || !this.cors || !window.AudioContext) return;
    try {
      this.ctx = new AudioContext();
      const src = this.ctx.createMediaElementSource(this.audio);
      this.an = this.ctx.createAnalyser(); this.an.fftSize = 2048; this.an.smoothingTimeConstant = 0.75;
      src.connect(this.an); this.an.connect(this.ctx.destination);
      this.bins = new Uint8Array(this.an.frequencyBinCount);
    } catch { this.ctx = null; }
  }
  paint() {
    const a = this.audio, cv = this.seekCv, dpr = devicePixelRatio || 1;
    const w = cv.clientWidth, h = cv.clientHeight;
    if (cv.width !== w * dpr) { cv.width = w * dpr; cv.height = h * dpr; }
    const g = cv.getContext("2d"); g.setTransform(dpr, 0, 0, dpr, 0, 0); g.clearRect(0, 0, w, h);
    const cs = getComputedStyle(this.root), cur = cs.getPropertyValue("--cur").trim();
    const col = cur.startsWith("var(") ? cs.getPropertyValue(cur.slice(4, -1)).trim() : cur || "#3fc1c9";
    const d = a.duration || 0, p = d ? a.currentTime / d : 0;
    // live spectrum across the whole bar (log frequency), dimmed; progress tints it
    if (this.an && !a.paused) this.an.getByteFrequencyData(this.bins);
    const n = 96, bw = w / n;
    for (let i = 0; i < n; i++) {
      const f0 = 40 * Math.pow(11000 / 40, i / n), bin = Math.min(this.bins?.length - 1 || 0, Math.round((f0 / ((this.ctx?.sampleRate || 44100) / 2)) * (this.bins?.length || 1)));
      const v = this.bins ? this.bins[bin] / 255 : 0;
      const bh = Math.max(1, v * v * (h - 6));
      g.fillStyle = i / n < p ? col : "#2c3139";
      g.globalAlpha = i / n < p ? 0.55 : 0.9;
      g.fillRect(i * bw + 1, h / 2 - bh / 2, bw - 2, bh);
    }
    g.globalAlpha = 1;
    // buffered + track line
    g.fillStyle = "#3a414c"; g.fillRect(0, h / 2 - 1, w, 2);
    if (d) for (let i = 0; i < a.buffered.length; i++) { g.fillStyle = "#5f6875"; g.fillRect((a.buffered.start(i) / d) * w, h / 2 - 1, ((a.buffered.end(i) - a.buffered.start(i)) / d) * w, 2); }
    g.fillStyle = col; g.fillRect(0, h / 2 - 1, p * w, 2);
    g.fillStyle = "#e7b84c"; g.shadowColor = "#e7b84c"; g.shadowBlur = 10; g.fillRect(p * w - 1, 2, 2, h - 4); g.shadowBlur = 0;
    this.timeEl.textContent = `${fmt(a.currentTime)} / ${fmt(d)}`;
    requestAnimationFrame(() => this.paint());
  }
}

export function transportScroll() {
  const t = document.getElementById("transport");
  if (!t || t.classList.contains("always")) return;
  const f = () => t.classList.toggle("solid", scrollY > 40);
  addEventListener("scroll", f, { passive: true }); f();
}
