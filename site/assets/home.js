// Landing page: hero oscilloscope (original vs model, per family), family rack, attack/loop diagram, live numbers.
(() => {
  const t = document.getElementById("transport");
  const f = () => t.classList.toggle("solid", scrollY > 40);
  addEventListener("scroll", f, { passive: true }); f();
})();
const $ = (s) => document.querySelector(s);
const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
const TAU = Math.PI * 2;

// ── tiny deterministic noise ──
let seed = 1;
const rnd = () => ((seed = (seed * 1664525 + 1013904223) >>> 0) / 4294967296) * 2 - 1;

// ── a "note" per family: f(x in periods, t in s, variant) → sample. variant 0 = original, 1 = model ──
const FAM = {
  additive: { name: "additive", from: "piano, organ, strings, reeds · 369 layers", f: (x, t, v) => {
    let s = 0; for (let k = 1; k <= 14; k++) { const a = Math.pow(k, -1.15) * Math.exp(-k * 0.05 * (1 + Math.sin(t * 0.7))); s += a * Math.sin(TAU * k * x * (1 + (v ? 0.0009 : 0.0012) * k * k) + k * 0.7 + (v ? 0.03 * k : 0)); }
    return s * 0.55; } },
  modal: { name: "modal", from: "bells, mallets, cymbals, toms · 157 layers", f: (x, t, v) => {
    const tt = (t % 1.6) + x * 0.004; const R = [1, 2.756, 5.404, 8.933, 13.34]; let s = 0;
    R.forEach((r, i) => (s += Math.exp(-tt * (2 + i * 2.2)) * Math.sin(TAU * r * (v ? 1.004 : 1) * x + i) / (1 + i * 0.6)));
    return s * 0.9; } },
  ks: { name: "Karplus-Strong", from: "guitars, basses, harp, sitar · 110 layers", f: null },
  fm: { name: "FM", from: "electric pianos, bells, tweets · 30 layers", f: (x, t, v) => {
    const I = 3.2 * Math.exp(-(t % 2) * 1.6) + 0.4; return 0.8 * Math.sin(TAU * x + I * (v ? 0.95 : 1) * Math.sin(TAU * 14 * x / 14 * (v ? 1 : 1) + 0.3) + 0.4 * Math.sin(TAU * x * 1)); } },
  shaker: { name: "shaker (PhISEM)", from: "maracas, cabasa, applause, water · 24 layers", f: null },
  formant: { name: "formant", from: "choir, voice oohs, laughter · 3 layers", f: (x, t, v) => {
    const F = [[730, 90], [1090, 110], [2440, 170]], f0 = 160; let s = 0;
    for (let k = 1; k <= 22; k++) { const f = k * f0; let g = 0; F.forEach(([c, b], i) => (g += 1 / (1 + Math.pow((f - c * (v ? 1.03 : 1)) / b, 2)) / (1 + i))); s += (g + 0.02) * Math.sin(TAU * k * x + k * k * 0.31) / Math.sqrt(k); }
    return s * 0.9; } },
};

// Karplus-Strong, really running (two strings: original / model with slightly different damping)
const ks = [0, 1].map((v) => ({ buf: new Float32Array(160), i: 0, damp: v ? 0.4985 : 0.4975, t: 0 }));
function pluck() { ks.forEach((s) => { seed = 7; for (let i = 0; i < s.buf.length; i++) s.buf[i] = rnd() * 0.9; }); }
pluck();
function ksStep(n) { ks.forEach((s) => { for (let j = 0; j < n; j++) { const a = s.buf[s.i], b = s.buf[(s.i + 1) % s.buf.length]; s.buf[s.i] = (a + b) * s.damp; s.i = (s.i + 1) % s.buf.length; } }); }
// shaker: decaying collision bursts
let shk = [];
function shakerFrame(t, v) { seed = Math.floor(t * 18) * 31 + (v ? 3 : 0); const out = new Float32Array(400); for (let c = 0; c < 9; c++) { const at = Math.floor(((rnd() + 1) / 2) * 400); const amp = (rnd() + 1.2) * 0.5 * Math.exp(-(t % 1.2) * 2.2); for (let i = at; i < 400; i++) { const d = Math.exp(-(i - at) / (v ? 11 : 9)); if (d < 0.01) break; out[i] += amp * d * rnd() * (0.6 + 0.4 * Math.sin(i * 0.9)); } } return out; }

let fam = "additive";
const cv = $("#scope"), g = cv.getContext("2d");
function drawScope(now) {
  const t = now / 1000, dpr = Math.min(2, devicePixelRatio || 1), w = cv.clientWidth, h = cv.clientHeight;
  if (cv.width !== Math.round(w * dpr)) { cv.width = Math.round(w * dpr); cv.height = Math.round(h * dpr); }
  g.setTransform(dpr, 0, 0, dpr, 0, 0);
  g.fillStyle = "rgba(14,16,19,0.34)"; g.fillRect(0, 0, w, h);
  const cy = h * 0.42, amp = h * 0.2, N = Math.min(900, Math.floor(w / 1.6));
  const trace = (v, col, lw) => {
    g.beginPath();
    for (let i = 0; i <= N; i++) {
      const x = i / N; let y;
      if (fam === "ks") { const s = ks[v]; y = s.buf[(s.i + Math.floor(x * 3 * s.buf.length)) % s.buf.length] * 1.4; }
      else if (fam === "shaker") { if (!shk[v]) shk[v] = shakerFrame(t, v); y = shk[v][Math.floor(x * 399)] * 1.2; }
      else y = FAM[fam].f(x * 3, t, v);
      const px = x * w, py = cy - y * amp;
      i ? g.lineTo(px, py) : g.moveTo(px, py);
    }
    g.strokeStyle = col; g.lineWidth = lw; g.shadowColor = col; g.shadowBlur = 12; g.stroke(); g.shadowBlur = 0;
  };
  trace(0, "rgba(240,160,75,0.55)", 2.4);
  trace(1, "rgba(63,193,201,0.95)", 1.4);
  if (fam === "ks") { ksStep(4); if ((t % 2.2) < 0.02) pluck(); }
  if (fam === "shaker") shk = [];
  if (!reduced) requestAnimationFrame(drawScope);
}
requestAnimationFrame(drawScope);
document.querySelectorAll(".pills button").forEach((b) => b.addEventListener("click", () => {
  fam = b.dataset.f; if (fam === "ks") pluck();
  document.querySelectorAll(".pills button").forEach((x) => x.setAttribute("aria-pressed", String(x === b)));
  $("#fam-name").textContent = FAM[fam].name; $("#fam-from").textContent = FAM[fam].from;
  if (reduced) requestAnimationFrame(drawScope);
}));
$("#fam-from").textContent = FAM.additive.from + " · orange: original, cyan: model";

// ── attack / loop diagram ──
(function wave() {
  const atk = [], lp = [], lp2 = [];
  const f = (i) => { let s = 0; for (let k = 1; k < 9; k++) s += Math.sin(i * 0.38 * k * (1 + 0.0008 * k * k) + k) / k; return s; };
  for (let x = 20; x <= 214; x += 1) { const i = x - 20; const e = Math.min(1, i / 6) * (1.6 * Math.exp(-i / 60) + 0.55) * (1 + 0.25 * Math.sin(i * 0.05)); atk.push(`${x},${120 - f(i) * 26 * e}`); }
  const L = 116;
  for (let x = 214; x <= 330; x++) lp.push(`${x},${120 - f(x - 214) * 26 * 0.55}`);
  for (let x = 330; x <= 540; x++) lp2.push(`${x},${120 - f((x - 214) % L) * 26 * 0.55}`);
  $("#atk").setAttribute("d", "M" + atk.join(" L")); $("#lp").setAttribute("d", "M" + lp.join(" L")); $("#lp2").setAttribute("d", "M" + lp2.join(" L"));
})();

// ── family rack ──
const RACK = [
  ["additive", "var(--b-g)", "w6", "Partials on the loop grid", "Harmonic table, inharmonicity, unison or Gaussian ensemble spread, vibrato and per-harmonic AM tables, sideband components, a brightness envelope. Sustain is exact; phases are Schroeder or hashed.", "piano, organ, strings, brass, reeds"],
  ["noise", "var(--voice)", "w3", "Shaped noise", "Up to 24 band-passes (third-octave cymbal washes), sweeps, AM. The loop is a loop-length noise pattern filtered circularly, so it repeats exactly.", "breath, cymbals, floor"],
  ["modal", "var(--b-o)", "w3", "Struck resonators", "Up to 32 modes [Hz, dB, decay, phase], hit trains or measured strikes, glides and trills per hit, a band-passed click.", "bells, mallets, toms"],
  ["ks", "var(--env)", "", "Plucked strings", "Extended Karplus-Strong: pick comb, dynamics low-pass, damping, dispersion all-pass tuned by bisection, a commuted body, sitar buzz.", "guitar, bass, harp, sitar"],
  ["fm", "var(--lfo)", "", "Operators", "Two to four operators as a stack or pairs, feedback, index envelopes, glide, and a pitch/loudness cycle for bird tweets. The loop snaps every ratio to whole cycles.", "EP, bells, tweets"],
  ["shaker", "var(--b-p)", "", "Collisions (PhISEM)", "Random collisions at a rate that follows the envelope, each a decaying burst through up to three resonances, with chirp and detune per collision.", "maracas, water, applause"],
  ["subtractive", "var(--filt)", "", "Two oscillators, one filter", "Saw, pulse or triangle as harmonic series through an RBJ filter evaluated per harmonic, with a cutoff envelope. Detuned pairs are placed by their beat phase.", "leads, square, synth bass"],
  ["formant", "var(--warn)", "", "Voices", "A saw or pulse source with jitter and breath through five parallel formants. Syllables have their own envelopes and pitch fall.", "choir, oohs, laughter"],
  ["drum + metal", "var(--err)", "", "Kicks, snares and 808 metal", "drum: a body gliding from freq × ratio, shape, drive, beater click and extra membrane modes. metal: six PolyBLEP squares (the TR-808 set by default) through band-pass and high-pass with a two-stage decay.", "kick, snare, hi-hat, cowbell"],
];
function mini(c, kind, col) {
  const d = Math.min(2, devicePixelRatio || 1), w = c.clientWidth, h = c.clientHeight; c.width = w * d; c.height = h * d;
  const x = c.getContext("2d"); x.scale(d, d); x.strokeStyle = col; x.lineWidth = 1.3; x.beginPath();
  for (let i = 0; i <= w; i++) {
    const p = i / w; let y = 0; seed = i * 13 + 5;
    if (kind === "additive") for (let k = 1; k < 10; k++) y += Math.sin(TAU * k * p * 4 + k) / k * 0.6;
    else if (kind === "noise") y = rnd() * 0.7 * (0.5 + 0.5 * Math.sin(p * 9));
    else if (kind === "modal") y = Math.exp(-p * 4) * (Math.sin(TAU * p * 22) + 0.6 * Math.sin(TAU * p * 61));
    else if (kind === "ks") y = Math.exp(-p * 2.5) * Math.sign(Math.sin(TAU * p * 9)) * (0.4 + 0.5 * Math.exp(-p * 8)) * (Math.sin(TAU * p * 9) ** 2) * 1.2;
    else if (kind === "fm") y = Math.sin(TAU * p * 5 + 3 * Math.exp(-p * 3) * Math.sin(TAU * p * 70));
    else if (kind === "shaker") y = (Math.sin(p * 41) > 0.7 ? 1 : 0.15) * rnd() * Math.exp(-((p * 7) % 1) * 3);
    else if (kind === "subtractive") y = 0.8 * (((p * 8) % 1) * 2 - 1) * (0.6 + 0.4 * Math.exp(-p * 3));
    else if (kind === "formant") y = Math.exp(-((p * 6) % 1) * 5) * Math.sin(TAU * p * 6 * 7) * 0.9;
    else y = i < w * 0.45 ? Math.exp(-p * 7) * Math.sin(TAU * (p * 6 + 5 * Math.exp(-p * 20))) : rnd() * 0.5 * Math.exp(-(p - 0.45) * 6);
    const py = h / 2 - y * h * 0.4; i ? x.lineTo(i, py) : x.moveTo(i, py);
  }
  x.stroke();
}
fetch("data/stats.json").then((r) => r.json()).then((st) => {
  const fam = st.gm.fam;
  const count = (k) => k === "drum + metal" ? (fam.drum || 0) + (fam.metal || 0) : fam[k] || 0;
  $("#rack").innerHTML = RACK.map(([k, c, cls, h, p, u]) => `<article class="dev ${cls} rv" style="--acc:${c}"><div class="devhd"><span class="led"></span>${k}</div>
    <div class="body"><h3>${h}</h3><p>${p}</p><canvas class="mini" data-k="${k}"></canvas><div class="ft"><b>${count(k)}</b>layers in gen-gm · ${u}</div></div></article>`).join("");
  const css = (v) => { const m = v.match(/var\((--[\w-]+)\)/); return m ? getComputedStyle(document.documentElement).getPropertyValue(m[1]).trim() : v; };
  document.querySelectorAll("canvas.mini").forEach((c) => mini(c, c.dataset.k, css(c.closest(".dev").style.getPropertyValue("--acc"))));
  $("#c-gm").innerHTML = `${st.gm.A}<small> / ${st.gm.B} / ${st.gm.C}</small>`;
  $("#c-plus").innerHTML = `${st.plus.A}<small> / ${st.plus.B} / ${st.plus.C}</small>`;
  $("#s-ab").textContent = `${st.gm.A + st.gm.B} / ${st.gm.n}`;
  $("#m-a").style.setProperty("--h", `${Math.round(((st.gm.A + st.gm.B) / st.gm.n) * 100)}%`);
  $("#cats").innerHTML = st.categories.map((c) => `<span>${c.cat}</span><span class="tr"><i class="g" data-w="${c.gm}"></i><i class="p" data-w="${c.plus}"></i></span><b>${c.gm.toFixed(0)}</b><b style="color:var(--b-p)">${c.plus.toFixed(0)}</b>`).join("");
  observe();
});
fetch("data/songs.json").then((r) => r.json()).then(({ songs }) => {
  const med = (a) => { const s = [...a].sort((x, y) => x - y); return s[s.length >> 1]; };
  $("#c-tf").innerHTML = `${med(songs.map((s) => s.mix.tf)).toFixed(1)}<small> dB</small>`;
  $("#c-tfs").innerHTML = `${med(songs.map((s) => s.mix.tfs)).toFixed(1)}<small> dB</small>`;
  const cols = ["var(--b-o)", "var(--b-s)", "var(--b-g)", "var(--b-p)"];
  $("#eq").innerHTML = songs.slice(0, 40).map((s, i) => `<b style="--c:${cols[i % 4]};height:${Math.max(8, 100 - s.mix.tf * 9)}%"></b>`).join("");
});

// ── reveal + bars ──
function observe() {
  const io = new IntersectionObserver((es) => es.forEach((e) => {
    if (!e.isIntersecting) return;
    e.target.classList.add("in");
    e.target.querySelectorAll("[data-w]").forEach((i) => (i.style.width = i.dataset.w + "%"));
    io.unobserve(e.target);
  }), { threshold: 0.12 });
  document.querySelectorAll(".rv:not(.in)").forEach((el) => io.observe(el));
}
observe();
