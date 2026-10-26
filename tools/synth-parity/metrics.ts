// Audio parity metrics shared by the synth parity tools (ws-parity, tf-parity): integrated loudness (ITU-R BS.1770-4),
// long-term average spectrum per octave band (63 Hz–16 kHz) and onset times. Sample-exact comparison is not the
// goal: long double-precision phase accumulators drift between libm implementations, so songs compare perceptually.
import { readFileSync, writeFileSync } from "node:fs";

export interface Audio {
  rate: number;
  ch: Float32Array[];
}

// ---------- WAV ----------

/** PCM 16-bit or IEEE float 32-bit WAV → per-channel floats. */
export function readWav(path: string): Audio {
  const b = readFileSync(path);
  const v = new DataView(b.buffer, b.byteOffset, b.byteLength);
  if (b.toString("ascii", 0, 4) !== "RIFF" || b.toString("ascii", 8, 12) !== "WAVE") throw new Error(`${path}: not WAV`);
  let fmt = 0, chans = 0, rate = 0, bits = 0, data = -1, size = 0;
  for (let o = 12; o + 8 <= b.length; ) {
    const id = b.toString("ascii", o, o + 4), n = v.getUint32(o + 4, true);
    if (id === "fmt ") [fmt, chans, rate, bits] = [v.getUint16(o + 8, true), v.getUint16(o + 10, true), v.getUint32(o + 12, true), v.getUint16(o + 22, true)];
    if (id === "data") [data, size] = [o + 8, Math.min(n, b.length - o - 8)];
    o += 8 + n + (n & 1);
  }
  const float = fmt === 3 && bits === 32;
  if (data < 0 || !(float || (fmt === 1 && bits === 16))) throw new Error(`${path}: need 16-bit PCM or 32-bit float`);
  const bps = bits / 8, frames = Math.floor(size / (bps * chans));
  const ch = Array.from({ length: chans }, () => new Float32Array(frames));
  for (let i = 0; i < frames; i++)
    for (let c = 0; c < chans; c++) {
      const o = data + (i * chans + c) * bps;
      ch[c][i] = float ? v.getFloat32(o, true) : v.getInt16(o, true) / 32768;
    }
  return { rate, ch };
}

export function writeWav(path: string, a: Audio): void {
  const n = a.ch[0].length, c = a.ch.length, b = Buffer.alloc(44 + n * c * 2);
  b.write("RIFF", 0, "ascii"); b.writeUInt32LE(36 + n * c * 2, 4); b.write("WAVEfmt ", 8, "ascii");
  b.writeUInt32LE(16, 16); b.writeUInt16LE(1, 20); b.writeUInt16LE(c, 22); b.writeUInt32LE(a.rate, 24);
  b.writeUInt32LE(a.rate * c * 2, 28); b.writeUInt16LE(c * 2, 32); b.writeUInt16LE(16, 34);
  b.write("data", 36, "ascii"); b.writeUInt32LE(n * c * 2, 40);
  for (let i = 0; i < n; i++)
    for (let k = 0; k < c; k++) b.writeInt16LE(Math.max(-32768, Math.min(32767, Math.round(a.ch[k][i] * 32767))), 44 + (i * c + k) * 2);
  writeFileSync(path, b);
}

// ---------- loudness ----------

type Biquad = [number, number, number, number, number]; // b0 b1 b2 a1 a2 (a0 = 1)

function norm(b0: number, b1: number, b2: number, a0: number, a1: number, a2: number): Biquad {
  return [b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0];
}

/** BS.1770 K-weighting (shelf + RLB high-pass) for any rate, in libebur128's bilinear design (exact at 48 kHz). */
function kWeighting(rate: number): Biquad[] {
  let K = Math.tan((Math.PI * 1681.974450955533) / rate);
  const Q = 0.7071752369554196, Vh = 10 ** (3.999843853973347 / 20), Vb = Vh ** 0.4996667741545416;
  const shelf = norm(Vh + (Vb * K) / Q + K * K, 2 * (K * K - Vh), Vh - (Vb * K) / Q + K * K, 1 + K / Q + K * K, 2 * (K * K - 1), 1 - K / Q + K * K);
  K = Math.tan((Math.PI * 38.13547087602444) / rate);
  const q = 0.5003270373238773;
  const hp = norm(1, -2, 1, 1 + K / q + K * K, 2 * (K * K - 1), 1 - K / q + K * K);
  return [shelf, hp];
}

function filter(x: Float32Array, fs: Biquad[]): Float64Array {
  const y = Float64Array.from(x);
  for (const [b0, b1, b2, a1, a2] of fs) {
    let x1 = 0, x2 = 0, y1 = 0, y2 = 0;
    for (let i = 0; i < y.length; i++) {
      const v = y[i], o = b0 * v + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
      [x2, x1, y2, y1] = [x1, v, y1, o];
      y[i] = o;
    }
  }
  return y;
}

/** Integrated loudness in LUFS: 400 ms blocks, 75 % overlap, −70 LUFS absolute and −10 LU relative gates. */
export function loudness(a: Audio): number {
  const k = a.ch.map((c) => filter(c, kWeighting(a.rate)));
  const block = Math.round(0.4 * a.rate), hop = Math.round(0.1 * a.rate);
  const z: number[] = [];
  for (let s = 0; s + block <= k[0].length; s += hop) {
    let sum = 0;
    for (const c of k) {
      let e = 0;
      for (let i = s; i < s + block; i++) e += c[i] * c[i];
      sum += e / block; // channel weights 1 (L, R)
    }
    z.push(sum);
  }
  const lk = (e: number) => -0.691 + 10 * Math.log10(e);
  const mean = (xs: number[]) => xs.reduce((p, x) => p + x, 0) / xs.length;
  const abs = z.filter((e) => lk(e) > -70);
  if (!abs.length) return -Infinity;
  const rel = lk(mean(abs)) - 10;
  const gated = abs.filter((e) => lk(e) > rel);
  return lk(mean(gated));
}

// ---------- spectrum ----------

function fft(re: Float64Array, im: Float64Array): void {
  const n = re.length;
  for (let i = 1, j = 0; i < n; i++) {
    let bit = n >> 1;
    for (; j & bit; bit >>= 1) j ^= bit;
    j ^= bit;
    if (i < j) [re[i], re[j], im[i], im[j]] = [re[j], re[i], im[j], im[i]];
  }
  for (let len = 2; len <= n; len <<= 1) {
    const ang = (-2 * Math.PI) / len, wr = Math.cos(ang), wi = Math.sin(ang);
    for (let i = 0; i < n; i += len) {
      let cr = 1, ci = 0;
      for (let j = 0; j < len / 2; j++) {
        const a = i + j, b = a + len / 2;
        const tr = re[b] * cr - im[b] * ci, ti = re[b] * ci + im[b] * cr;
        [re[b], im[b], re[a], im[a]] = [re[a] - tr, im[a] - ti, re[a] + tr, im[a] + ti];
        [cr, ci] = [cr * wr - ci * wi, cr * wi + ci * wr];
      }
    }
  }
}

function mono(a: Audio): Float32Array {
  const m = new Float32Array(a.ch[0].length);
  for (const c of a.ch) for (let i = 0; i < m.length; i++) m[i] += c[i] / a.ch.length;
  return m;
}

export const OCTAVES = [63, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];

/** Long-term average power per octave band (dB, bands [fc/√2, fc·√2)), Hann frames of 8192, hop 4096. */
export function octaveLtas(a: Audio): { fc: number; db: number }[] {
  const N = 8192, m = mono(a), win = Float64Array.from({ length: N }, (_, i) => 0.5 - 0.5 * Math.cos((2 * Math.PI * i) / N));
  const power = new Float64Array(N / 2);
  let frames = 0;
  for (let s = 0; s + N <= Math.max(m.length, N); s += N / 2, frames++) {
    const re = new Float64Array(N), im = new Float64Array(N);
    for (let i = 0; i < N; i++) re[i] = (m[s + i] ?? 0) * win[i];
    fft(re, im);
    for (let k = 0; k < N / 2; k++) power[k] += re[k] * re[k] + im[k] * im[k];
  }
  return OCTAVES.map((fc) => {
    let e = 0;
    for (let k = 0; k < N / 2; k++) {
      const f = (k * a.rate) / N;
      if (f >= fc / Math.SQRT2 && f < fc * Math.SQRT2) e += power[k];
    }
    return { fc, db: 10 * Math.log10(e / frames + 1e-30) };
  });
}

// ---------- onsets ----------

/**
 * Onset frames: spectral flux (positive log-magnitude rise, 1024-point Hann, hop 64) peak-picked against a local
 * mean (±0.25 s) plus a floor relative to the song's strongest flux; peaks closer than 1024 frames merge. Times carry a constant
 * detector bias (about −N/2), which cancels when two renders are compared.
 */
export function onsets(a: Audio): number[] {
  const H = FLUX_HOP;
  const flux = onsetFlux(a);
  const max = flux.reduce((p, x) => Math.max(p, x), 0);
  return pickOnsets(flux, max, a.rate, H);
}

/** Hop of the spectral-flux curve, in frames. */
export const FLUX_HOP = 64;

/** Spectral flux of `a` (log-magnitude increases summed over bins), one value per `FLUX_HOP` frames. */
export function onsetFlux(a: Audio): Float64Array {
  const N = 1024, H = FLUX_HOP, m = mono(a), win = Float64Array.from({ length: N }, (_, i) => 0.5 - 0.5 * Math.cos((2 * Math.PI * i) / N));
  // frames stop where the window would leave the signal (zero padding there reads as a click)
  const frames = Math.max(1, Math.floor((m.length - N / 2) / H) + 1);
  const flux = new Float64Array(frames);
  let prev = new Float64Array(N / 2).fill(Math.log(1e-6));
  for (let f = 0; f < frames; f++) {
    const re = new Float64Array(N), im = new Float64Array(N), s = f * H - N / 2; // frame centred on f·H
    for (let i = 0; i < N; i++) re[i] = (m[s + i] ?? 0) * win[i];
    fft(re, im);
    const cur = new Float64Array(N / 2);
    let d = 0;
    for (let k = 1; k < N / 2; k++) {
      cur[k] = Math.log(Math.hypot(re[k], im[k]) + 1e-6);
      d += Math.max(0, cur[k] - prev[k]);
    }
    flux[f] = d;
    prev = cur;
  }
  return flux;
}

function pickOnsets(flux: Float64Array, max: number, rate: number, H: number): number[] {
  const frames = flux.length;
  const W = Math.round((0.25 * rate) / H), gap = 1024 / H, out: number[] = [];
  const pre = new Float64Array(frames + 1);
  for (let f = 0; f < frames; f++) pre[f + 1] = pre[f] + flux[f];
  for (let f = 0; f < frames; f++) {
    const lo = Math.max(0, f - W), hi = Math.min(frames, f + W + 1);
    const local = (pre[hi] - pre[lo]) / (hi - lo);
    let peak = flux[f] > local * 1.5 + 0.1 * max;
    for (let g = Math.max(0, f - 2); peak && g <= Math.min(frames - 1, f + 2); g++) if (flux[g] > flux[f]) peak = false;
    if (peak && (!out.length || f - out[out.length - 1] / H >= gap)) out.push(f * H);
  }
  return out;
}

/** Rectified spectral flux minus its local mean (±0.25 s): the onset-strength curve the alignment correlates. */
function novelty(flux: Float64Array, rate: number): Float64Array {
  const W = Math.round((0.25 * rate) / FLUX_HOP), pre = new Float64Array(flux.length + 1);
  for (let f = 0; f < flux.length; f++) pre[f + 1] = pre[f] + flux[f];
  return Float64Array.from(flux, (x, f) => {
    const lo = Math.max(0, f - W), hi = Math.min(flux.length, f + W + 1);
    return Math.max(0, x - (pre[hi] - pre[lo]) / (hi - lo));
  });
}

/**
 * Timing of `ours` against `ref` by the cross-correlation of their onset-strength curves: the lag in frames (positive =
 * ours later, multiple of `FLUX_HOP`) with the highest normalised correlation within ±`maxLag`, and that correlation.
 * Unlike peak picking it does not depend on which of two neighbouring hits each render's threshold happens to keep.
 */
export function alignment(ours: Audio, ref: Audio, maxLag: number): { lag: number; corr: number; at0: number } {
  const [x, y] = [novelty(onsetFlux(ours), ours.rate), novelty(onsetFlux(ref), ref.rate)];
  const L = Math.ceil(maxLag / FLUX_HOP);
  const dot = (lag: number) => {
    let s = 0;
    for (let f = Math.max(0, -lag); f < y.length && f + lag < x.length; f++) s += x[f + lag] * y[f];
    return s;
  };
  const norm = Math.sqrt(x.reduce((p, v) => p + v * v, 0) * y.reduce((p, v) => p + v * v, 0)) || 1;
  let best = 0, bestC = -Infinity;
  for (let lag = -L; lag <= L; lag++) {
    const c = dot(lag) / norm;
    if (c > bestC) [best, bestC] = [lag, c];
  }
  return { lag: best * FLUX_HOP, corr: bestC, at0: dot(0) / norm };
}

// ---------- comparison ----------

export interface Tolerance {
  loudnessDb: number;
  bandDb: number;
  /** Onset match window in frames (WaveSabre: one 100-frame step; Tunefish: one 512-frame block). */
  onsetFrames: number;
  /** Fraction of onsets that must match, in each direction. */
  onsetMatch: number;
  /** Bands more than this many dB below the loudest reference band are skipped (default `BAND_FLOOR_DB`). */
  bandFloorDb?: number;
  /**
   * Set: the onset check passes when the onset-strength curves of the two renders correlate at least this much with a
   * lag within `onsetFrames` (see `alignment`), instead of matching peak-picked onsets. For songs of two different
   * engines the peak picker keeps a different one of two neighbouring hits in each render, which is not a timing error.
   */
  onsetAlignment?: number;
}

export const DEFAULT_TOL: Tolerance = { loudnessDb: 1, bandDb: 3, onsetFrames: 100, onsetMatch: 0.95 };

/** Bands quieter than this (dB below the loudest band) are skipped: their level is noise, not music. */
export const BAND_FLOOR_DB = 60;

export interface Report {
  loudness: { ours: number; ref: number; diff: number; pass: boolean };
  bands: { fc: number; ours: number; ref: number; diff: number; pass: boolean }[];
  onsets: { total: number; matched: number; extra: number; maxOffset: number; pass: boolean; lag?: number; corr?: number };
  pass: boolean;
}

function matchCount(from: number[], to: number[], tol: number): { n: number; max: number } {
  let n = 0, max = 0, j = 0;
  for (const f of from) {
    while (j + 1 < to.length && Math.abs(to[j + 1] - f) <= Math.abs(to[j] - f)) j++;
    const d = to.length ? Math.abs(to[j] - f) : Infinity;
    if (d <= tol) [n, max] = [n + 1, Math.max(max, d)];
  }
  return { n, max };
}

export function compare(ours: Audio, ref: Audio, tol: Tolerance = DEFAULT_TOL): Report {
  const [lo, lr] = [loudness(ours), loudness(ref)];
  const l = { ours: lo, ref: lr, diff: lo - lr, pass: Math.abs(lo - lr) <= tol.loudnessDb || (lo === lr) };
  const [bo, br] = [octaveLtas(ours), octaveLtas(ref)];
  const top = Math.max(...br.map((b) => b.db));
  const bands = bo.map((b, i) => {
    const diff = b.db - br[i].db;
    return { fc: b.fc, ours: b.db, ref: br[i].db, diff, pass: br[i].db < top - (tol.bandFloorDb ?? BAND_FLOOR_DB) || Math.abs(diff) <= tol.bandDb };
  });
  const [oo, or] = [onsets(ours), onsets(ref)];
  const fwd = matchCount(or, oo, tol.onsetFrames), back = matchCount(oo, or, tol.onsetFrames);
  const frac = (k: number, n: number) => (n === 0 ? 1 : k / n);
  const o = {
    total: or.length, matched: fwd.n, extra: oo.length - back.n, maxOffset: fwd.max,
    pass: frac(fwd.n, or.length) >= tol.onsetMatch && frac(back.n, oo.length) >= tol.onsetMatch,
    lag: undefined as number | undefined, corr: undefined as number | undefined,
  };
  if (tol.onsetAlignment !== undefined) {
    const al = alignment(ours, ref, Math.max(1024, 2 * tol.onsetFrames));
    Object.assign(o, { lag: al.lag, corr: al.corr, pass: Math.abs(al.lag) <= tol.onsetFrames && al.corr >= tol.onsetAlignment });
  }
  return { loudness: l, bands, onsets: o, pass: l.pass && bands.every((b) => b.pass) && o.pass };
}

export function formatReport(name: string, r: Report): string {
  const f = (x: number) => (Number.isFinite(x) ? x.toFixed(2) : String(x));
  const lines = [
    `${name}: ${r.pass ? "PASS" : "FAIL"}`,
    `  loudness ours ${f(r.loudness.ours)} ref ${f(r.loudness.ref)} LUFS, diff ${f(r.loudness.diff)} dB ${r.loudness.pass ? "ok" : "FAIL"}`,
    `  octaves ${r.bands.map((b) => `${b.fc}:${f(b.diff)}${b.pass ? "" : "!"}`).join(" ")}`,
    `  onsets ${r.onsets.matched}/${r.onsets.total} matched, ${r.onsets.extra} extra, max offset ${r.onsets.maxOffset} frames${r.onsets.lag === undefined ? "" : `, alignment lag ${r.onsets.lag} frames corr ${f(r.onsets.corr!)}`} ${r.onsets.pass ? "ok" : "FAIL"}`,
  ];
  return lines.join("\n");
}
