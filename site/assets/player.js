// The listening deck: four banks of the same music, switched at the same position.
//   gm.dls (copyrighted) plays as a pre-rendered audio file; gmplus, gen-gm and gen-plus play live from the MIDI
//   (live.js: SpessaSynth + gmgen.wasm, the banks built in the browser). All four go through one analyser.
// Keys: 1–4 bank, Space play/pause, ← → seek 5 s, L loop.
import { Live, built } from "./live.js";

export const BANKS = [
  { k: "o", name: "gm.dls", sub: "original · audio", c: "var(--b-o)" },
  { k: "s", name: "gmplus", sub: "original · live", c: "var(--b-s)" },
  { k: "g", name: "gen-gm", sub: "procedural · live", c: "var(--b-g)" },
  { k: "p", name: "gen-plus", sub: "procedural · live", c: "var(--b-p)" },
];
export const NOTE = { hold: 1.6, total: 2.6 };
const fmt = (t) => (Number.isFinite(t) ? `${Math.floor(t / 60)}:${String(Math.floor(t % 60)).padStart(2, "0")}` : "0:00");
const midiCache = new Map();
const midiBytes = (url) => { if (!midiCache.has(url)) midiCache.set(url, fetch(url).then((r) => r.arrayBuffer())); return midiCache.get(url); };

export class Deck {
  constructor(root, { onEnded } = {}) {
    this.root = root; this.onEnded = onEnded;
    this.bank = localStorage.getItem("gmp.bank2") || "o";
    this.item = null; this.playing = false; this.pos = 0; this.loopOn = false; this.busy = null;
    this.audio = new Audio(); this.audio.crossOrigin = "anonymous"; this.audio.preload = "auto";
    root.querySelector(".banks").innerHTML = BANKS.map((b, i) =>
      `<button data-k="${b.k}" style="--c:${b.c}" aria-pressed="false" title="key ${i + 1}">${b.name}<small>${i + 1} · ${b.sub}</small></button>`).join("");
    this.btns = [...root.querySelectorAll(".banks button")];
    this.btns.forEach((b) => b.addEventListener("click", () => this.setBank(b.dataset.k)));
    this.playBtn = root.querySelector(".play"); this.playBtn.addEventListener("click", () => this.toggle());
    this.timeEl = root.querySelector(".time"); this.subEl = root.querySelector(".now .b");
    this.seekEl = root.querySelector(".seek"); this.seekCv = this.seekEl.querySelector("canvas");
    this.loopBtn = root.querySelector(".loop");
    this.loopBtn?.addEventListener("click", () => this.setLoop(!this.loopOn));
    const seekTo = (e) => { const r = this.seekEl.getBoundingClientRect(); const d = this.duration(); if (d) this.seek(Math.max(0, Math.min(d, ((e.clientX - r.left) / r.width) * d))); };
    let drag = false;
    this.seekEl.addEventListener("pointerdown", (e) => { drag = true; this.seekEl.setPointerCapture(e.pointerId); seekTo(e); });
    this.seekEl.addEventListener("pointermove", (e) => drag && seekTo(e));
    this.seekEl.addEventListener("pointerup", () => (drag = false));
    this.audio.addEventListener("ended", () => this.ended());
    addEventListener("keydown", (e) => {
      if (e.target.closest("input, select, textarea") || e.metaKey || e.ctrlKey || e.altKey) return;
      if (e.key >= "1" && e.key <= "4") { this.setBank(BANKS[+e.key - 1].k); e.preventDefault(); }
      else if (e.key === " ") { this.toggle(); e.preventDefault(); }
      else if (e.key === "ArrowLeft") { this.seek(Math.max(0, this.position() - 5)); e.preventDefault(); }
      else if (e.key === "ArrowRight") { this.seek(Math.min(this.duration(), this.position() + 5)); e.preventDefault(); }
      else if (e.key === "l" || e.key === "L") this.setLoop(!this.loopOn);
    });
    this.mark(); this.paint();
  }

  // ── audio graph (created on the first user gesture) ──
  graph() {
    if (this.ctx) return;
    this.ctx = new AudioContext();
    this.an = this.ctx.createAnalyser(); this.an.fftSize = 2048; this.an.smoothingTimeConstant = 0.75;
    this.an.connect(this.ctx.destination);
    this.bins = new Uint8Array(this.an.frequencyBinCount);
    try { this.ctx.createMediaElementSource(this.audio).connect(this.an); } catch { /* plays directly, no spectrum */ }
    this.live = new Live(this.ctx, this.an);
  }

  /** item: { kind: "song", title, by, mp3, midi, seconds, solo } | { kind: "note", title, by, mp3, note } */
  load(item, { autoplay = true } = {}) {
    this.stopAll();
    this.item = item; this.pos = 0;
    this.root.querySelector(".now .t").textContent = item.title;
    this.subEl.textContent = item.by || "";
    if (item.kind === "note") this.setLoop(true, true);
    if (autoplay) this.play(0); else this.audio.removeAttribute("src");
  }
  setBank(k) {
    if (k === this.bank) return;
    const t = this.position(), was = this.playing;
    this.stopAll();
    this.bank = k; localStorage.setItem("gmp.bank2", k); this.mark();
    this.pos = t;
    if (was && this.item) this.play(t);
  }
  setLoop(on, quiet) { this.loopOn = on; this.audio.loop = on && this.item?.kind === "note"; this.loopBtn?.setAttribute("aria-pressed", String(on)); }
  duration() { return !this.item ? 0 : this.item.kind === "note" ? NOTE.total : this.bank === "o" && this.audio.duration ? this.audio.duration : this.item.seconds + 2; }
  position() {
    if (!this.item) return 0;
    if (!this.playing) return this.pos;
    if (this.bank === "o") return this.audio.currentTime;
    if (this.item.kind === "note") return (this.ctx.currentTime - this.t0) % NOTE.total;
    return this.live.seq?.currentTime ?? this.pos;
  }
  toggle() { if (!this.item) return; this.playing || this.busy ? this.pause() : this.play(this.pos); }
  pause() { this.pos = this.position(); this.stopAll(); }
  seek(t) { this.pos = t; if (this.playing) this.play(t); }
  ended() { if (this.item?.kind === "song" && !this.loopOn) { this.stopAll(); this.pos = 0; this.onEnded?.(); } else if (this.loopOn) this.play(0); }

  stopAll() {
    this.playing = false; this.busy = null; this.progress = null;
    this.audio.pause();
    this.live?.stop();
    clearInterval(this.retrig); this.cancelNote?.();
    this.btnState();
  }
  async play(at) {
    this.graph(); await this.ctx.resume();
    this.stopAll();
    const item = this.item, bank = this.bank, token = (this.busy = {});
    this.btnState();
    if (bank === "o") {
      if (this.audio.src !== item.mp3) { this.audio.src = item.mp3; await new Promise((r) => this.audio.addEventListener("loadedmetadata", r, { once: true })); }
      if (this.busy !== token) return;
      this.audio.loop = this.loopOn && item.kind === "note";
      this.audio.currentTime = Math.min(at, (this.audio.duration || at + 1) - 0.05);
      await this.audio.play().catch(() => {});
    } else {
      try {
        await this.live.use(bank, ({ label, frac }) => { if (this.busy === token) { this.subEl.textContent = label; this.progress = frac; } });
      } catch (e) { this.subEl.textContent = `could not load the bank: ${e.message}`; this.busy = null; this.btnState(); return; }
      if (this.busy !== token) return;
      this.progress = null; this.subEl.textContent = item.by || "";
      if (built[bank] && !this.toldBuilt?.[bank]) { (this.toldBuilt ??= {})[bank] = 1; this.subEl.textContent = `${BANKS.find((b) => b.k === bank).name} built in your browser in ${(built[bank] / 1000).toFixed(1)} s · ${item.by || ""}`; }
      if (item.kind === "note") {
        const fire = () => { this.t0 = this.ctx.currentTime; this.cancelNote = this.live.note(item.note, NOTE.hold); };
        fire();
        this.retrig = setInterval(() => (this.loopOn ? fire() : (this.stopAll(), (this.pos = 0))), NOTE.total * 1000);
      } else {
        const bytes = await midiBytes(item.midi);
        if (this.busy !== token) return;
        if (this.live.midiUrl !== item.midi) { this.live.load(bytes); this.live.midiUrl = item.midi; }
        this.live.solo(item.solo ?? null);
        this.live.seq.currentTime = at;
        this.live.seq.play();
      }
    }
    if (this.busy !== token) return;
    this.busy = null; this.playing = true; this.btnState();
  }
  btnState() {
    const on = this.playing || this.busy;
    this.playBtn.textContent = on ? "❚❚" : "▶"; this.playBtn.setAttribute("aria-label", on ? "Pause" : "Play");
  }
  mark() { this.btns.forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.k === this.bank))); this.root.style.setProperty("--cur", BANKS.find((b) => b.k === this.bank).c); }

  paint() {
    const cv = this.seekCv, dpr = devicePixelRatio || 1, w = cv.clientWidth, h = cv.clientHeight;
    if (cv.width !== w * dpr) { cv.width = w * dpr; cv.height = h * dpr; }
    const g = cv.getContext("2d"); g.setTransform(dpr, 0, 0, dpr, 0, 0); g.clearRect(0, 0, w, h);
    const name = BANKS.find((b) => b.k === this.bank).c.slice(4, -1);
    const col = getComputedStyle(document.documentElement).getPropertyValue(name).trim() || "#3fc1c9";
    const d = this.duration(), t = this.position(), p = d ? Math.min(1, t / d) : 0;
    // live song end
    if (this.playing && this.item?.kind === "song" && this.bank !== "o" && t >= this.item.seconds + 1.9) this.ended();
    if (this.an && this.playing) this.an.getByteFrequencyData(this.bins);
    const n = 96, bw = w / n;
    for (let i = 0; i < n; i++) {
      const f0 = 40 * Math.pow(11000 / 40, i / n), bin = this.bins ? Math.min(this.bins.length - 1, Math.round((f0 / (this.ctx.sampleRate / 2)) * this.bins.length)) : 0;
      const v = this.bins && this.playing ? this.bins[bin] / 255 : 0, bh = Math.max(1, v * v * (h - 6));
      g.fillStyle = i / n < p ? col : "#2c3139"; g.globalAlpha = i / n < p ? 0.55 : 0.9;
      g.fillRect(i * bw + 1, h / 2 - bh / 2, bw - 2, bh);
    }
    g.globalAlpha = 1;
    g.fillStyle = "#3a414c"; g.fillRect(0, h / 2 - 1, w, 2);
    if (this.progress != null) { g.fillStyle = col; g.globalAlpha = 0.35; g.fillRect(0, 2, this.progress * w, h - 4); g.globalAlpha = 1; }
    g.fillStyle = col; g.fillRect(0, h / 2 - 1, p * w, 2);
    g.fillStyle = "#e7b84c"; g.shadowColor = "#e7b84c"; g.shadowBlur = 10; g.fillRect(p * w - 1, 2, 2, h - 4); g.shadowBlur = 0;
    this.timeEl.textContent = `${fmt(t)} / ${fmt(d)}`;
    requestAnimationFrame(() => this.paint());
  }
}

export function transportScroll() {
  const t = document.getElementById("transport");
  if (!t || t.classList.contains("always")) return;
  const f = () => t.classList.toggle("solid", scrollY > 40);
  addEventListener("scroll", f, { passive: true }); f();
}
