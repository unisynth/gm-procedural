// Time × band distance between two renders of the same MIDI: third-octave band levels per 46 ms frame (mono),
// compared after removing the overall gain difference, so it measures timbre and balance over time, not level.
import type { Audio } from "./metrics.ts";

const N = 2048, HOP = 1024;
export const BANDS = Array.from({ length: 26 }, (_, i) => 50 * 2 ** (i / 3)); // 50 Hz … 16 kHz

function fft(re: Float64Array, im: Float64Array) {
  const n = re.length;
  for (let i = 1, j = 0; i < n; i++) {
    let b = n >> 1;
    for (; j & b; b >>= 1) j ^= b;
    j |= b;
    if (i < j) { [re[i], re[j]] = [re[j], re[i]]; [im[i], im[j]] = [im[j], im[i]]; }
  }
  for (let len = 2; len <= n; len <<= 1) {
    const a = (-2 * Math.PI) / len, wr = Math.cos(a), wi = Math.sin(a);
    for (let i = 0; i < n; i += len) {
      let cr = 1, ci = 0;
      for (let k = 0; k < len / 2; k++) {
        const p = i + k, q = p + len / 2, tr = re[q] * cr - im[q] * ci, ti = re[q] * ci + im[q] * cr;
        re[q] = re[p] - tr; im[q] = im[p] - ti; re[p] += tr; im[p] += ti;
        [cr, ci] = [cr * wr - ci * wi, cr * wi + ci * wr];
      }
    }
  }
}

/** Band energies (dB) per frame: rows = frames, columns = BANDS. */
export function bandFrames(a: Audio): Float64Array[] {
  const n = a.ch[0].length, win = Float64Array.from({ length: N }, (_, i) => 0.5 - 0.5 * Math.cos((2 * Math.PI * i) / N));
  const edge = BANDS.map((f) => [f / 2 ** (1 / 6), f * 2 ** (1 / 6)].map((x) => Math.round((x * N) / a.rate)));
  const out: Float64Array[] = [];
  const re = new Float64Array(N), im = new Float64Array(N);
  for (let s = 0; s + N <= n; s += HOP) {
    for (let i = 0; i < N; i++) {
      let m = 0;
      for (const c of a.ch) m += c[s + i];
      re[i] = (m / a.ch.length) * win[i];
      im[i] = 0;
    }
    fft(re, im);
    out.push(Float64Array.from(edge, ([lo, hi]) => {
      let e = 1e-12;
      for (let k = Math.max(1, lo); k <= Math.min(hi, N / 2); k++) e += re[k] * re[k] + im[k] * im[k];
      return 10 * Math.log10(e);
    }));
  }
  return out;
}

export type Dist = {
  /** mean |Δ dB| over active frames × relevant bands, after the global gain offset */
  tf: number;
  /** global level offset gen − orig (dB, energy over active frames) */
  gain: number;
  /** mean Δ dB per band (gen − orig, after gain), for tilt */
  bands: number[];
};

const sum = (f: Float64Array) => 10 * Math.log10(f.reduce((s, d) => s + 10 ** (d / 10), 0));

/** Distance of `gen` from `orig`. Frames > 45 dB below the loudest orig frame and bands > 50 dB below the frame's
 * loudest band (in either render) are ignored; both are floored 60 dB below the frame's loudest band. */
export function dist(gen: Audio, orig: Audio): Dist {
  const g = bandFrames(gen), o = bandFrames(orig), n = Math.min(g.length, o.length);
  const tot = o.slice(0, n).map(sum), top = Math.max(...tot);
  const act = [...Array(n).keys()].filter((i) => tot[i] > top - 45);
  if (!act.length) return { tf: NaN, gain: NaN, bands: BANDS.map(() => NaN) };
  const e = (fs: Float64Array[]) => act.reduce((s, i) => s + 10 ** (sum(fs[i]) / 10), 0);
  const gain = 10 * Math.log10(e(g) / e(o));
  let d = 0, cnt = 0;
  const bs = BANDS.map(() => 0), bc = BANDS.map(() => 0);
  for (const i of act) {
    const fm = Math.max(...o[i], ...g[i].map((x) => x - gain));
    for (let b = 0; b < BANDS.length; b++) {
      const ov = Math.max(o[i][b], fm - 60), gv = Math.max(g[i][b] - gain, fm - 60);
      if (Math.max(ov, gv) < fm - 50) continue;
      d += Math.abs(gv - ov); cnt++;
      bs[b] += gv - ov; bc[b]++;
    }
  }
  return { tf: d / cnt, gain, bands: bs.map((s, b) => (bc[b] ? s / bc[b] : 0)) };
}
