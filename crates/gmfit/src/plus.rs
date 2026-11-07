//! gmplus targets (change add-gmplus-bank, D2): for each gm.dls wave, the sample of the gmplus zone that plays the
//! wave's key (`main_user`), DC-blocked, through the zone's static low-pass, resampled to 32 kHz so that played by
//! the gm.dls region at that key it sounds as gmplus does, with an integer loop (whole source loops, the residual
//! pitch error going to the fine tune) and level-matched to the gm.dls wave. Waves without a gmplus zone fall back
//! to the gm.dls wave at 32 kHz. `assets/gm-plus/map.json` records every choice (no audio).

use crate::dsp;
use crate::sf2::{gn, Sf2};
use gmgen::render::WaveMeta;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::OnceLock;

/// Rate of every plus wave.
pub const RATE: u32 = 32000;
/// Note-on velocity the zones and modulators are resolved at.
pub const VELOCITY: u8 = 100;
/// Most whole source loops a target loop may hold.
const MAX_PERIODS: usize = 256;
/// Longest target loop (samples).
const MAX_LOOP: usize = 16384;
/// Highest target peak (below the generator's 0.9 soft knee).
pub const PEAK: f32 = 0.85;
/// Longest unlooped target (s), unless the gm.dls wave is longer.
const MAX_ONESHOT_SECS: f64 = 3.0;

/// A target wave and its provenance row.
#[derive(Clone, Debug)]
pub struct Target {
    pub samples: Vec<f32>,
    /// `(start, length)` in target samples.
    pub looped: Option<(u32, u32)>,
    /// Cents added to the gm.dls wave's (and its regions') fine tune.
    pub fine_delta: i16,
    /// dB added to the wsmp gain of the wave (and its regions): the headroom taken off the samples.
    pub wsmp_gain_db: f32,
    /// Right channel when gmplus pans the wave's zones apart (design D4): the target of its partner wave.
    pub right: Option<Vec<f32>>,
    pub row: Value,
}

impl Target {
    pub fn meta(&self, gm: &WaveMeta) -> WaveMeta {
        WaveMeta { rate: RATE, len: self.samples.len(), unity: gm.unity, fine: gm.fine + self.fine_delta, looped: self.looped }
    }
}

pub struct Targets {
    sf: Sf2,
    cache: Vec<OnceLock<Result<Target, String>>>,
    /// Waves whose zones are panned apart, in index order; partner of `stereo[k]` is wave `GM_WAVES + k`.
    stereo: OnceLock<Vec<usize>>,
}

/// Waves of the gm.dls skeleton; partner waves follow them.
pub const GM_WAVES: usize = 495;
/// Pan spread (SF2 units, 0.1 %) above which a wave's zones count as panned apart.
const PAN_SPREAD: f32 = 1.0;

/// One channel of a stereo wave: every zone's gain times its pan gain for that side.
fn side(srcs: &[Source], right: bool) -> Vec<Source> {
    srcs.iter()
        .cloned()
        .map(|mut z| {
            let (pl, pr) = pan_gains(z.pan);
            z.gain *= if right { pr } else { pl };
            z
        })
        .collect()
}

/// Equal-power pan gains (left, right) for SF2 pan −500..500 (FluidSynth).
fn pan_gains(pan: f64) -> (f64, f64) {
    let th = ((pan.clamp(-500.0, 500.0) + 500.0) / 1000.0) * std::f64::consts::FRAC_PI_2;
    (th.cos(), th.sin())
}

fn gm_skeleton() -> &'static dls::write::Bank {
    static B: OnceLock<dls::write::Bank> = OnceLock::new();
    B.get_or_init(gmgen::bank::skeleton)
}

/// A-weighted RMS (FFT power weighted by `score::a_weight`).
pub fn aw_rms(x: &[f32], rate: f32) -> f32 {
    use rustfft::{num_complex::Complex, FftPlanner};
    if x.is_empty() {
        return 0.0;
    }
    let n = x.len().next_power_of_two();
    let mut buf: Vec<Complex<f32>> = x.iter().map(|&v| Complex::new(v, 0.0)).chain(std::iter::repeat(Complex::new(0.0, 0.0))).take(n).collect();
    FftPlanner::new().plan_fft_forward(n).process(&mut buf);
    let mut e = 0.0f64;
    for (k, c) in buf.iter().enumerate().take(n / 2 + 1).skip(1) {
        let f = k as f32 * rate / n as f32;
        e += c.norm_sqr() as f64 * 10f64.powf(crate::score::a_weight(f) as f64 / 10.0) * if k == n / 2 { 1.0 } else { 2.0 };
    }
    ((e / n as f64 / x.len() as f64).sqrt()) as f32
}

/// Loudest 100 ms window (hop 50 ms) of `x` by A-weighted RMS; the whole of `x` when shorter.
fn loudest(x: &[f32], rate: f32) -> f32 {
    let (w, hop) = ((0.1 * rate) as usize, (0.05 * rate) as usize);
    if x.len() <= w {
        return aw_rms(x, rate);
    }
    (0..=(x.len() - w) / hop).map(|k| aw_rms(&x[k * hop..k * hop + w], rate)).fold(0.0, f32::max)
}

/// SoundFont modulation envelope (SF2 2.04 §8.1.2 gens 25–32) in seconds; value 0..1.
#[derive(Clone, Copy, Debug, Default)]
struct ModEnv {
    delay: f64,
    attack: f64,
    hold: f64,
    decay: f64,
    sustain: f64,
}

impl ModEnv {
    fn from_gens(g: &[i32; gn::COUNT], key: u8) -> ModEnv {
        let secs = |tc: f64| (tc / 1200.0).exp2();
        let ks = 60.0 - key as f64;
        ModEnv {
            delay: secs(g[25] as f64),
            attack: secs(g[26] as f64),
            hold: secs(g[27] as f64 + g[31] as f64 * ks),
            decay: secs(g[28] as f64 + g[32] as f64 * ks),
            sustain: (1.0 - g[29] as f64 / 1000.0).clamp(0.0, 1.0),
        }
    }
    fn value(&self, t: f64) -> f64 {
        let (a, h) = (self.delay + self.attack, self.delay + self.attack + self.hold);
        if t < self.delay {
            0.0
        } else if t < a {
            (t - self.delay) / self.attack
        } else if t < h {
            1.0
        } else {
            (1.0 - (t - h) / self.decay).max(self.sustain)
        }
    }
    /// Time from which the value stays at the sustain level.
    fn settled(&self) -> f64 {
        self.delay + self.attack + self.hold + self.decay * (1.0 - self.sustain)
    }
}

/// Zone low-pass in the played domain: cutoff `base + env · value` cents, resonance `q` cB.
#[derive(Clone, Copy, Debug)]
struct Filter {
    base: f64,
    env: f64,
    q: f64,
}

/// One zone of a target before resampling.
#[derive(Clone)]
struct Source {
    x: Vec<f32>,
    rate: f64,
    /// Read step in source samples per target sample with the pitch envelope at 0.
    step: f64,
    /// `(start, length)` in source samples.
    looped: Option<(usize, usize)>,
    /// Play speed of the wave at its key: target sample n sounds at time n / (RATE · speed).
    speed: f64,
    env: ModEnv,
    /// Cents of pitch at envelope value 1 (`modEnvToPitch`).
    pitch_env: f64,
    filter: Option<Filter>,
    /// Linear gain of the zone (`initialAttenuation`, cB): layered zones keep their balance.
    gain: f64,
    /// SF2 pan of the zone (−500..500); used only for stereo waves.
    pan: f64,
}

impl Source {
    fn t_of(&self, n: usize) -> f64 {
        n as f64 / (RATE as f64 * self.speed)
    }
    fn bend(&self, n: usize) -> f64 {
        (self.pitch_env * self.env.value(self.t_of(n)) / 1200.0).exp2()
    }
    fn moving(&self) -> bool {
        self.pitch_env != 0.0 || self.filter.is_some_and(|f| f.env != 0.0)
    }
    /// DC removed: a looped sample loses its loop's mean (exact for a periodic loop, and no high-pass start-up
    /// transient repeated in an early loop); a one-shot goes through the 10 Hz high-pass.
    fn x_dc(&self) -> Vec<f32> {
        match self.looped {
            Some((s0, p)) => {
                let mean = self.x[s0..s0 + p].iter().map(|&v| v as f64).sum::<f64>() / p as f64;
                self.x.iter().map(|&v| v - mean as f32).collect()
            }
            None => dsp::dc_block(&self.x, self.rate),
        }
    }
    /// Variable-step read positions for target samples `0..n` (pitch envelope applied).
    fn positions(&self, n: usize) -> Vec<f64> {
        let mut pos = Vec::with_capacity(n);
        let mut q = 0.0;
        for k in 0..n {
            pos.push(q);
            q += self.step * self.bend(k);
        }
        pos
    }
    /// First target sample past the source loop start with the envelope settled (≤ `MAX_FREEZE`).
    fn attack_end(&self) -> usize {
        let freeze_t = if self.moving() { self.env.settled().min(MAX_FREEZE) } else { 0.0 };
        let s0 = self.looped.map_or(0, |l| l.0) as f64;
        let (mut n, mut q) = (0usize, 0.0f64);
        while q < s0 || self.t_of(n) < freeze_t {
            q += self.step * self.bend(n);
            n += 1;
        }
        n
    }
    /// The zone's low-pass in the played domain; the envelope stops at `freeze`.
    fn filt(&self, y: &[f32], freeze: Option<usize>) -> Vec<f32> {
        let Some(f) = self.filter else { return y.to_vec() };
        let shift = 1200.0 * self.speed.log2();
        let lo = f.base.min(f.base + f.env) - shift;
        if lo >= 13500.0 || dsp::cents_hz(lo) >= 0.45 * RATE as f64 {
            return y.to_vec();
        }
        dsp::run_varying(y, 16, |n| {
            let n = freeze.map_or(n, |a| n.min(a));
            dsp::sf_lowpass_clamped(f.base + f.env * self.env.value(self.t_of(n)) - shift, f.q, RATE as f64)
        })
    }
    /// One-shot render of `n` target samples (stops at the sample end).
    fn oneshot(&self, n: usize) -> Vec<f32> {
        let x = self.x_dc();
        let pos: Vec<f64> = self.positions(n).into_iter().take_while(|&q| q < (x.len() - 1) as f64).collect();
        let mut y = self.filt(&dsp::resample_at(&x, &pos), None);
        // 10 ms fade at the end: a zone cut by the length cap, or a short one under a looped zone, ends without a click.
        let f = (RATE as usize / 100).min(y.len());
        let n = y.len();
        y[n - f..].iter_mut().enumerate().for_each(|(k, v)| *v *= 1.0 - (k + 1) as f32 / f as f32);
        y
    }
}

/// Longest baked attack (s, played time): mod-envelope motion beyond it is frozen into the loop.
const MAX_FREEZE: f64 = 1.0;

/// Kernel half width in source samples at read step `step`, plus a guard.
fn kernel_reach(step: f64) -> usize {
    (TAPS_HALF * step.max(1.0)).ceil() as usize + 8
}

/// Renders a target from its zones (summed with their gains, as gmplus layers them). The mod envelopes bend pitch and
/// filter until they settle (≤ `MAX_FREEZE`, past every source loop start), then all freeze at the same sample `a0`
/// for one integer loop `L` that holds whole source loops of every looped zone (`m_k·p_k/L` per zone, searched up to
/// `MAX_PERIODS` primary loops, first with every zone within 0.25 cent). One-shot zones of a looped target end
/// before the loop (faded over 10 ms if longer). Returns samples, loop, the primary zone's residual cents (→ fine
/// tune), the worst zone's residual cents, and the freeze time (s).
fn render(zs: &[Source], oneshot_len: usize) -> (Vec<f32>, Option<(u32, u32)>, f64, f64, f64, usize) {
    let mix = |parts: Vec<(f64, Vec<f32>)>| -> Vec<f32> {
        let n = parts.iter().map(|p| p.1.len()).max().unwrap_or(0);
        let mut out = vec![0.0f32; n];
        for (g, y) in parts {
            out.iter_mut().zip(&y).for_each(|(o, v)| *o += (g as f32) * v);
        }
        out
    };
    let looped: Vec<&Source> = zs.iter().filter(|z| z.looped.is_some()).collect();
    if looped.is_empty() {
        return (mix(zs.iter().map(|z| (z.gain, z.oneshot(oneshot_len.max(1)))).collect()), None, 0.0, 0.0, 0.0, 0);
    }
    let mut a0 = looped.iter().map(|z| z.attack_end()).max().unwrap();
    // One-shot zones play out before the loop when they are short; longer ones fade out at the loop's attack end.
    let shots: Vec<(f64, Vec<f32>)> = zs.iter().filter(|z| z.looped.is_none()).map(|z| (z.gain, z.oneshot(oneshot_len.max(1)))).collect();
    let longest = shots.iter().map(|s| s.1.len()).max().unwrap_or(0);
    a0 = a0.max(longest.min(a0 + RATE as usize / 2));
    // Loop: L target samples = m_k source loops of every looped zone.
    let step_l: Vec<f64> = looped.iter().map(|z| z.step * z.bend(a0)).collect();
    let (p0, sl0) = (looped[0].looped.unwrap().1, step_l[0]);
    let fit = |l: usize| -> (Vec<usize>, Vec<f64>) {
        looped
            .iter()
            .zip(&step_l)
            .map(|(z, &st)| {
                let p = z.looped.unwrap().1;
                let m = ((l as f64 * st / p as f64).round() as usize).max(1);
                (m, 1200.0 * ((m * p) as f64 / l as f64 / st).log2())
            })
            .unzip()
    };
    let worst = |d: &[f64]| d.iter().fold(0.0f64, |w, v| w.max(v.abs()));
    // The primary (first looped) zone sets the loop: whole source loops of it, up to MAX_LOOP (or one source loop
    // when that alone is longer). Among loops within 0.5 cent for the primary, the one that locks the most other
    // zones within 1 cent wins, then the smallest residual. Zones that do not lock keep their exact pitch and are
    // crossfaded across the loop end (detuned unisons, noise layers, fifths).
    let lock = |d: f64| d.abs() <= 1.0;
    let rank = |devs: &[f64]| (devs[1..].iter().filter(|d| !lock(**d)).count(), devs[0].abs() > 0.5);
    let mut best: Option<(usize, Vec<usize>, Vec<f64>)> = None;
    for m in 1..=MAX_PERIODS {
        let l = ((m * p0) as f64 / sl0).round() as usize;
        if l < 2 {
            continue;
        }
        if l > MAX_LOOP && best.is_some() {
            break;
        }
        let (ms, devs) = fit(l);
        let better = best.as_ref().is_none_or(|b| {
            let (r, br) = (rank(&devs), rank(&b.2));
            (r.1, r.0) < (br.1, br.0) || ((r.1, r.0) == (br.1, br.0) && worst(&devs) < worst(&b.2) - 1e-9)
        });
        if better {
            best = Some((l, ms, devs));
        }
        let b = best.as_ref().unwrap();
        if rank(&b.2) == (0, false) && worst(&b.2) <= 0.25 {
            break;
        }
        if l > MAX_LOOP {
            break;
        }
    }
    let Some((l, ms, devs)) = best else {
        // No loop of ≥ 2 samples within MAX_PERIODS source loops: play the zones as one-shots.
        return (mix(zs.iter().map(|z| (z.gain, z.oneshot(oneshot_len.max(1)))).collect()), None, 0.0, 0.0, 0.0, 0);
    };
    // Locked zones read at m·p/L; the primary always (its residual goes into the fine tune); the rest at their own step.
    let locked: Vec<bool> = devs.iter().enumerate().map(|(k, &d)| k == 0 || lock(d)).collect();
    let steps: Vec<f64> = (0..looped.len())
        .map(|k| if locked[k] { (ms[k] * looped[k].looped.unwrap().1) as f64 / l as f64 } else { step_l[k] })
        .collect();
    let xfade = locked.iter().any(|&k| !k);
    let a = a0 + steps.iter().map(|&s| kernel_reach(s)).max().unwrap();
    // Kept period: after ≥ 4096 samples of loop (filters settle) when every zone is locked (the attack ends at `a`,
    // so its length does not matter); after whole loops covering ≥ 2048 when a crossfaded zone makes the attack
    // run up to the kept period.
    let settle = if xfade { 2048usize.div_ceil(l).max(1) } else { 1 + 4096usize.div_ceil(l) };
    let want = a + (settle + 1) * l;
    let last = a + settle * l;
    let fade = (RATE as usize / 50).min(l / 2).max(1);
    let mut parts: Vec<(f64, Vec<f32>)> = looped
        .iter()
        .zip(&steps)
        .zip(&locked)
        .map(|((z, &step), &lk)| {
            let (s0, p) = z.looped.unwrap();
            let x = z.x_dc();
            let mut pos = z.positions(a0 + 1);
            let p0 = pos[a0];
            pos.truncate(a0);
            pos.extend((a0..want).map(|n| p0 + (n - a0) as f64 * step));
            // Source periodically extended past the loop, as far as the kernel reads.
            let need = pos[pos.len() - 1] as usize + kernel_reach(step);
            let mut src = x[..(s0 + p).min(x.len())].to_vec();
            while src.len() < need {
                let k = (src.len() - s0) % p;
                src.push(x[s0 + k]);
            }
            let mut y = z.filt(&dsp::resample_at(&src, &pos), Some(a0));
            if !lk {
                // Equal-power crossfade of the loop's end into the audio before the loop start: the wrap continues
                // as the sound did into the loop.
                for k in 0..fade {
                    let w = (k + 1) as f64 / fade as f64 * std::f64::consts::FRAC_PI_2;
                    let n = last + l - fade + k;
                    y[n] = (w.cos() * y[n] as f64 + w.sin() * y[n - l] as f64) as f32;
                }
            }
            (z.gain, y)
        })
        .collect();
    for (g, mut y) in shots {
        if y.len() > a0 {
            y.truncate(a0);
            let f = (RATE as usize / 100).min(y.len());
            let n = y.len();
            y[n - f..].iter_mut().enumerate().for_each(|(k, v)| *v *= 1.0 - (k + 1) as f32 / f as f32);
        }
        parts.push((g, y));
    }
    let y = mix(parts);
    // With crossfaded zones the attack runs up to the kept period, so the loop is entered continuously.
    let mut out = y[..if xfade { last } else { a }].to_vec();
    out.extend_from_slice(&y[last..last + l]);
    let freeze = if looped.iter().any(|z| z.moving()) { looped[0].t_of(a0) } else { 0.0 };
    let loop_start = out.len() - l;
    let locked_dev = devs.iter().zip(&locked).filter(|(_, k)| **k).map(|(d, _)| *d).collect::<Vec<_>>();
    (out, Some((loop_start as u32, l as u32)), devs[0], worst(&locked_dev), freeze, locked.iter().filter(|&&k| !k).count())
}


const TAPS_HALF: f64 = (dsp::TAPS / 2) as f64 / 0.95;

impl Targets {
    /// Opens the soundfont named by `GMPLUS_SF2`.
    pub fn load() -> Result<Targets, String> {
        let p = std::env::var("GMPLUS_SF2").ok().filter(|p| !p.is_empty()).ok_or(
            "the plus set needs the soundfont: set GMPLUS_SF2 to a local copy of gmplus.sf2 (never commit it)",
        )?;
        let sf = Sf2::open(std::path::Path::new(&p)).map_err(|e| format!("GMPLUS_SF2={p}: {e}"))?;
        // Room for every wave and a partner each; `total()` is the real count.
        Ok(Targets { sf, cache: (0..2 * GM_WAVES).map(|_| OnceLock::new()).collect(), stereo: OnceLock::new() })
    }

    /// Zones gmplus plays for wave `i` (< `GM_WAVES`) at its main user's key.
    fn zones_of(&self, i: usize) -> Option<(crate::User, Vec<crate::sf2::Resolved>)> {
        let bank = gm_skeleton();
        let gm_meta = gmgen::bank::meta(i)?;
        let u = crate::main_user(bank, Some((gm_meta.unity, gm_meta.fine)), i)?;
        let ins = &bank.instruments[u.ins];
        let (pb, pp) = if ins.drums { (128, ins.program as u16) } else { (ins.bank_msb as u16, ins.program as u16) };
        Some((u, self.sf.zones(pb, pp, u.key, VELOCITY)))
    }

    /// Waves whose gmplus zones are panned apart (design D4), in index order.
    pub fn stereo(&self) -> &[usize] {
        self.stereo.get_or_init(|| {
            (0..GM_WAVES)
                .filter(|&i| {
                    self.zones_of(i).is_some_and(|(u, zs)| {
                        let pans: Vec<f32> = zs.iter().map(|z| z.with_mods(gn::PAN, VELOCITY, u.key)).collect();
                        zs.len() > 1 && pans.iter().fold(f32::MIN, |a, &b| a.max(b)) - pans.iter().fold(f32::MAX, |a, &b| a.min(b)) > PAN_SPREAD
                    })
                })
                .collect()
        })
    }

    /// Waves of the plus set: the skeleton's, then one partner per stereo wave.
    pub fn total(&self) -> usize {
        GM_WAVES + self.stereo().len()
    }

    /// Left wave of partner `i`, if `i` is a partner.
    pub fn left_of(&self, i: usize) -> Option<usize> {
        i.checked_sub(GM_WAVES).and_then(|k| self.stereo().get(k).copied())
    }

    /// Target of wave `i` (built once per process). `gm` is the Windows bank (fallback source and level reference).
    pub fn wave(&self, gm: &dls::Dls, i: usize) -> Result<Target, String> {
        if i >= self.total() {
            return Err(format!("wave {i} out of range"));
        }
        self.cache[i].get_or_init(|| match self.left_of(i) {
            // Partner: the right channel of its left wave, same loop, tuning and gain.
            Some(l) => self.wave(gm, l).and_then(|t| {
                let right = t.right.clone().ok_or(format!("wave {l}: no right channel"))?;
                Ok(Target { samples: right, right: None, row: json!({"wave": i, "left": l, "len": t.samples.len(), "loop": t.looped}), ..t })
            }),
            None => self.build(gm, i),
        })
        .clone()
    }

    fn build(&self, gm: &dls::Dls, i: usize) -> Result<Target, String> {
        let gm_meta = gmgen::bank::meta(i).ok_or(format!("wave {i}: no meta"))?;
        let g = crate::audio::original_wave(gm, i);
        let bank = gm_skeleton();
        let user = crate::main_user(bank, Some((gm_meta.unity, gm_meta.fine)), i);
        let gm_rate = gm_meta.rate as f64;
        let mut row = json!({"wave": i});
        let zones = user.map(|u| {
            let ins = &bank.instruments[u.ins];
            let (pb, pp) = if ins.drums { (128, ins.program as u16) } else { (ins.bank_msb as u16, ins.program as u16) };
            row["preset"] = json!([pb, pp]);
            row["key"] = json!(u.key);
            (u, self.sf.zones(pb, pp, u.key, VELOCITY))
        });
        let srcs: Vec<Source> = match zones {
            Some((u, zs)) if !zs.is_empty() => {
                let mut out = Vec::new();
                let mut layers = Vec::new();
                for z in &zs {
                    let smp = &self.sf.samples[z.sample];
                    let (a, e, ls, le) = z.bounds(smp);
                    let a = a.max(0) as usize;
                    let e = (e.max(0) as usize).max(a);
                    let x = self.sf.slice(a, e);
                    if x.len() < 2 {
                        return Err(format!("wave {i}: zone {} sample {} is empty", z.zone, smp.name));
                    }
                    let looped = (z.looped() && le > ls + 1 && ls >= a as i64 && le as usize <= e).then(|| ((ls as usize - a), (le - ls) as usize));
                    if looped.is_some_and(|(s0, p)| s0 + p > x.len()) {
                        return Err(format!("wave {i}: zone {} loop {ls}..{le} past the sample data ({} samples)", z.zone, x.len()));
                    }
                    let cents = z.cents(smp, u.key) as f64;
                    let step = (cents / 1200.0).exp2() * smp.rate as f64 / RATE as f64 / u.speed as f64;
                    let base = z.with_mods(gn::FILTER_FC, VELOCITY, u.key) as f64;
                    let fenv = z.with_mods(gn::MOD_ENV_TO_FC, VELOCITY, u.key) as f64;
                    let filter = (base < 13500.0 || fenv < 0.0).then_some(Filter { base, env: fenv, q: z.with_mods(gn::FILTER_Q, VELOCITY, u.key) as f64 });
                    // E-mu scaling of initialAttenuation (as FluidSynth, ALT_ATTENUATION_SCALE 0.4): soundfonts are
                    // voiced for it; read literally, Ice Rain's own sample (550 cB) would sit 55 dB under its pad.
                    let att = 0.4 * z.with_mods(gn::ATTENUATION, VELOCITY, u.key) as f64;
                    let pitch_env = z.with_mods(gn::MOD_ENV_TO_PITCH, VELOCITY, u.key) as f64;
                    if out.is_empty() {
                        row["preset_name"] = json!(z.preset);
                        row["instrument"] = json!(z.instrument);
                        row["zone"] = json!(z.zone);
                        row["sample"] = json!(smp.name);
                        row["src_rate"] = json!(smp.rate);
                        row["cents"] = json!(cents);
                        row["filter"] = json!(filter.map(|f| [dsp::cents_hz(f.base).round(), f.env, f.q]));
                        row["pitch_env"] = json!(pitch_env);
                    }
                    let pan = z.with_mods(gn::PAN, VELOCITY, u.key);
                    layers.push(json!({"zone": z.zone, "sample": smp.name, "cents": cents, "att_cb": att, "pan": pan, "looped": looped.is_some()}));
                    let env = ModEnv::from_gens(&z.gens, u.key);
                    out.push(Source { x, rate: smp.rate as f64, step, looped, speed: u.speed as f64, env, pitch_env, filter, gain: 10f64.powf(-att / 200.0), pan: pan as f64 });
                }
                // Zone gains relative to the loudest zone (the level match sets the absolute level).
                let g0 = out.iter().fold(0.0f64, |m, z| m.max(z.gain));
                out.iter_mut().for_each(|z| z.gain /= g0);
                row["zones"] = json!(zs.len());
                if zs.len() > 1 {
                    row["layers"] = json!(layers);
                }
                row["fallback"] = json!(false);
                out
            }
            _ => {
                row["fallback"] = json!(true);
                let speed = user.map_or(1.0, |u| u.speed as f64);
                vec![Source {
                    x: g.clone(),
                    rate: gm_rate,
                    step: gm_rate / RATE as f64,
                    looped: crate::audio::loop_of(&gm_meta),
                    speed,
                    env: ModEnv::default(),
                    pitch_env: 0.0,
                    filter: None,
                    gain: 1.0,
                    pan: 0.0,
                }]
            }
        };
        // Provenance (step, periods) from the zone that sets the loop: the first looped one.
        let src = srcs.iter().find(|z| z.looped.is_some()).unwrap_or(&srcs[0]);
        let oneshot = ((MAX_ONESHOT_SECS * RATE as f64) as usize).max((gm_meta.len as f64 * RATE as f64 / gm_rate) as usize);
        let stereo = self.stereo().binary_search(&i).ok();
        let (mut y, looped, dev, worst_dev, freeze, xfaded, mut yr) = match stereo {
            None => {
                let (y, l, d, w, f, x) = render(&srcs, oneshot);
                (y, l, d, w, f, x, None)
            }
            // Both channels on one timeline (the loop search does not depend on gains): equal length and loop.
            Some(_) => {
                let (y, l, d, w, f, x) = render(&side(&srcs, false), oneshot);
                let (yr, lr, ..) = render(&side(&srcs, true), oneshot);
                if (yr.len(), lr) != (y.len(), l) {
                    return Err(format!("wave {i}: stereo channels differ in length or loop"));
                }
                (y, l, d, w, f, x, Some(yr))
            }
        };
        row["freeze_s"] = json!((freeze * 1000.0).round() / 1000.0);
        let fine_delta = -(dev.round() as i16);
        let mut t = Target { samples: Vec::new(), looped, fine_delta, wsmp_gain_db: 0.0, right: None, row: Value::Null };
        // Level: the loudest 100 ms (A-weighted RMS) of the first 1 s as played (held note), matched to the gm.dls
        // wave. A first-window match over-boosts sounds whose filter opens slowly (+19 dB on 434, +30 dB on 301).
        t.samples = y.clone();
        let pm = t.meta(&gm_meta);
        let first = |x: Vec<f32>, rate: f64| x[..x.len().min(rate as usize)].to_vec();
        // Stereo: the louder channel takes the match; one gain keeps the image.
        let lv = |x: &[f32]| loudest(&first(crate::audio::played(x, &pm, 1.0), RATE as f64), RATE as f32);
        let (ra, rb) = (
            loudest(&first(crate::audio::played(&g, &gm_meta, 1.0), gm_rate), gm_rate as f32),
            lv(&y).max(yr.as_deref().map_or(0.0, lv)),
        );
        let gain = if rb > 1e-9 && ra > 1e-9 { ra / rb } else { 1.0 };
        // Peaks above the generator's soft knee move into the wsmp gain (same loudness in the bank).
        let peak = y.iter().chain(yr.iter().flatten()).fold(0.0f32, |m, v| m.max(v.abs())) * gain;
        let head = if peak > PEAK { PEAK / peak } else { 1.0 };
        y.iter_mut().chain(yr.iter_mut().flatten()).for_each(|v| *v *= gain * head);
        t.wsmp_gain_db = -20.0 * head.log10();
        row["step"] = json!(src.step);
        row["periods"] = json!(looped.map(|(_, l)| (l as f64 * src.step / src.looped.map_or(1, |p| p.1) as f64).round()));
        row["loop"] = json!(looped);
        row["len"] = json!(y.len());
        row["dev_cents"] = json!((dev * 1000.0).round() / 1000.0);
        if srcs.len() > 1 {
            row["locked_dev_cents"] = json!((worst_dev * 1000.0).round() / 1000.0);
            row["xfade_zones"] = json!(xfaded);
        }
        row["fine_delta"] = json!(fine_delta);
        if let Some(k) = stereo {
            row["stereo"] = json!({"partner": GM_WAVES + k});
        }
        row["gain_db"] = json!((20.0 * gain.log10() * 100.0).round() / 100.0);
        row["peak"] = json!((peak * 1000.0).round() / 1000.0);
        row["wsmp_gain_db"] = json!((t.wsmp_gain_db * 100.0).round() / 100.0);
        row["wsmp_gain"] = json!((t.wsmp_gain_db * GAIN_UNITS_PER_DB).round() as i32);
        t.samples = y;
        t.right = yr;
        t.row = row;
        Ok(t)
    }

    /// Writes `assets/gm-plus/map.json` (one row per wave, no audio) and returns its path and a summary.
    pub fn write_map(&self, gm: &dls::Dls) -> Result<(PathBuf, Value), String> {
        let n = self.total();
        let rows: Vec<Value> = std::thread::scope(|sc| {
            let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
            let hs: Vec<_> = (0..threads)
                .map(|t| sc.spawn(move || (t..n).step_by(threads).map(|i| (i, self.wave(gm, i))).collect::<Vec<_>>()))
                .collect();
            let mut all: Vec<(usize, Result<Target, String>)> = hs.into_iter().flat_map(|h| h.join().unwrap()).collect();
            all.sort_by_key(|x| x.0);
            all.into_iter().map(|(i, r)| r.map(|t| t.row).unwrap_or_else(|e| json!({"wave": i, "error": e}))).collect()
        });
        let count = |f: &dyn Fn(&Value) -> bool| rows.iter().filter(|r| f(r)).count();
        let summary = json!({
            "waves": rows.len(),
            "fallback": count(&|r| r["fallback"] == true),
            "errors": count(&|r| r.get("error").is_some()),
            "multi_zone": count(&|r| r["zones"].as_u64().is_some_and(|z| z > 1)),
            "looped": count(&|r| !r["loop"].is_null()),
            "max_dev_cents": rows.iter().filter_map(|r| r["dev_cents"].as_f64()).fold(0.0, |m: f64, v| m.max(v.abs())),
            "max_locked_dev_cents": rows.iter().filter_map(|r| r["locked_dev_cents"].as_f64()).fold(0.0, |m: f64, v| m.max(v.abs())),
            "xfaded": count(&|r| r["xfade_zones"].as_u64().is_some_and(|n| n > 0)),
            // gain left in the bank after the headroom move; `gain_db` only measures how quiet the raw sample is
            "stereo": self.stereo().len(),
            "loud_wsmp": rows.iter().filter(|r| r["wsmp_gain_db"].as_f64().is_some_and(|g| g > 12.0)).map(|r| r["wave"].clone()).collect::<Vec<_>>(),
            "headroom": count(&|r| r["wsmp_gain_db"].as_f64().is_some_and(|g| g > 0.0)),
            "pitch_env": count(&|r| r["pitch_env"].as_f64().is_some_and(|c| c != 0.0)),
            "filter_env": count(&|r| r["filter"][1].as_f64().is_some_and(|c| c != 0.0)),
        });
        let dir = crate::repo_root().join("assets/gm-plus");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let lines: Vec<String> = rows.iter().map(|r| r.to_string()).collect();
        let text = format!("{{\"format\": \"gm-plus-map/1\", \"summary\": {summary},\n\"waves\": [\n{}\n]}}\n", lines.join(",\n"));
        let path = dir.join("map.json");
        std::fs::write(&path, text).map_err(|e| e.to_string())?;
        Ok((path, summary))
    }
}

/// DLS `wsmp` gain units per dB (`lGain` is in 1/655360 dB).
pub const GAIN_UNITS_PER_DB: f32 = 655360.0;

impl Targets {
    /// The plus structure (design D3): the gm.dls structure with every wave at 32 kHz, its length and loop from the
    /// target, its fine tune shifted by `fine_delta` and its gain raised by `wsmp_gain_db`; region `wsmp` overrides get
    /// the same deltas as their wave.
    pub fn structure(&self, gm: &dls::Dls) -> Result<dls::write::Bank, String> {
        let mut b = dls::write::Bank::read(gm);
        let mut deltas = Vec::with_capacity(b.waves.len());
        for (i, w) in b.waves.iter_mut().enumerate() {
            let t = self.wave(gm, i)?;
            let dg = (t.wsmp_gain_db * GAIN_UNITS_PER_DB).round() as i32;
            w.rate = RATE;
            w.samples = vec![0; t.samples.len()];
            w.wsmp.fine_tune += t.fine_delta;
            w.wsmp.gain += dg;
            w.wsmp.looped = t.looped;
            deltas.push((t.fine_delta, dg));
        }
        for ins in &mut b.instruments {
            for r in &mut ins.regions {
                let (df, dg) = deltas[r.wave as usize];
                if let Some(ws) = r.wsmp.as_mut() {
                    ws.fine_tune += df;
                    ws.gain += dg;
                    // In gm.dls every region loop equals its wave's: take the target's (looped or not).
                    ws.looped = b.waves[r.wave as usize].wsmp.looped;
                }
            }
        }
        // Stereo partners (design D4/D5): appended after the skeleton, never referenced by a region; same rate,
        // length, loop, tuning and gain as their left wave.
        for (k, &l) in self.stereo().iter().enumerate() {
            let w = b.waves[l].clone();
            b.waves.push(w);
            b.pairs.push((l as u32, (GM_WAVES + k) as u32));
        }
        Ok(b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saw(period: f64, n: usize) -> Vec<f32> {
        (0..n).map(|k| ((k as f64 / period).fract() * 2.0 - 1.0) as f32 * 0.5).collect()
    }

    #[test]
    fn loop_is_integer_and_periodic() {
        // A 73.4-sample-period source loop of exactly 4 periods (293.6 → use 294 samples, period 73.5).
        let x = saw(73.5, 4000);
        let s = Source { x, rate: 32000.0, step: 0.91234, looped: Some((1000, 294)), speed: 1.0, env: ModEnv::default(), pitch_env: 0.0, filter: None, gain: 1.0, pan: 0.0 };
        let (y, lp, dev, _, _, _) = render(std::slice::from_ref(&s), 0);
        let (a, l) = lp.unwrap();
        assert!(dev.abs() <= 3.0, "dev {dev}");
        assert_eq!(y.len(), (a + l) as usize);
        // The loop wraps smoothly: the sample after the end continues the waveform at the loop start.
        let (a, l) = (a as usize, l as usize);
        let jump = (y[a + l - 1] - y[a]).abs();
        let typical = (1..l).map(|k| (y[a + k] - y[a + k - 1]).abs()).fold(0.0f32, f32::max);
        assert!(jump <= typical * 1.5 + 1e-3, "loop seam {jump} vs step {typical}");
    }

    #[test]
    fn oneshot_length_capped() {
        let s = Source { x: saw(50.0, 10000), rate: 44100.0, step: 44100.0 / 32000.0, looped: None, speed: 1.0, env: ModEnv::default(), pitch_env: 0.0, filter: None, gain: 1.0, pan: 0.0 };
        let (y, lp, _, _, _, _) = render(std::slice::from_ref(&s), 1000);
        assert!(lp.is_none());
        assert_eq!(y.len(), 1000);
    }

    #[test]
    fn mod_env_shape() {
        let e = ModEnv { delay: 0.1, attack: 0.1, hold: 0.1, decay: 1.0, sustain: 0.25 };
        assert_eq!(e.value(0.05), 0.0);
        assert!((e.value(0.15) - 0.5).abs() < 1e-9);
        assert_eq!(e.value(0.25), 1.0);
        assert!((e.value(0.8) - 0.5).abs() < 1e-9);
        assert_eq!(e.value(5.0), 0.25);
        assert!((e.settled() - 1.05).abs() < 1e-9);
    }

    #[test]
    fn filter_sweep_freezes_into_an_exact_loop() {
        let x = saw(64.0, 4000);
        let env = ModEnv { delay: 0.0, attack: 0.001, hold: 0.0, decay: 0.5, sustain: 0.0 };
        let s = Source { x, rate: 32000.0, step: 1.0, looped: Some((512, 640)), speed: 1.0, env, pitch_env: 0.0, filter: Some(Filter { base: 7000.0, env: 3600.0, q: 0.0 }), gain: 1.0, pan: 0.0 };
        let (y, lp, dev, _, freeze, _) = render(std::slice::from_ref(&s), 0);
        let (a, l) = lp.unwrap();
        assert!((freeze - 0.5).abs() < 0.01, "freeze {freeze}");
        assert!(a as f64 >= 0.5 * 32000.0, "attack holds the sweep");
        assert!(dev.abs() < 1e-9);
        // Bright early, darker in the loop.
        let hf = |v: &[f32]| v.windows(2).map(|w| (w[1] - w[0]).powi(2)).sum::<f32>() / v.iter().map(|x| x * x).sum::<f32>();
        assert!(hf(&y[2000..2640]) > hf(&y[a as usize..(a + l) as usize]) * 1.5);
    }

    #[test]
    fn pitch_envelope_bends_the_attack() {
        let x: Vec<f32> = (0..20000).map(|k| (std::f32::consts::TAU * k as f32 / 100.0).sin()).collect();
        let env = ModEnv { delay: 0.0, attack: 0.001, hold: 0.0, decay: 0.2, sustain: 0.0 };
        let s = Source { x, rate: 32000.0, step: 1.0, looped: Some((1000, 100)), speed: 1.0, env, pitch_env: 1200.0, filter: None, gain: 1.0, pan: 0.0 };
        let (y, lp, _, _, _, _) = render(std::slice::from_ref(&s), 0);
        let (a, l) = lp.unwrap();
        let crossings = |v: &[f32]| v.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        // Early: an octave up (period 50); loop: period 100 (l multiple of 100).
        assert!(crossings(&y[64..1064]) >= 18, "{}", crossings(&y[64..1064]));
        assert_eq!(l % 100, 0);
        assert!(crossings(&y[a as usize..(a + l) as usize]) == (l / 100) as usize);
    }

    #[test]
    fn aw_rms_of_1k_sine_is_its_rms() {
        let x: Vec<f32> = (0..32000).map(|k| (std::f32::consts::TAU * 1000.0 * k as f32 / 32000.0).sin()).collect();
        let r = aw_rms(&x, 32000.0);
        assert!((r / std::f32::consts::FRAC_1_SQRT_2 - 1.0).abs() < 0.03, "{r}");
    }

    #[test]
    fn missing_soundfont_names_the_variable() {
        if std::env::var("GMPLUS_SF2").is_ok() {
            return;
        }
        let e = Targets::load().err().unwrap();
        assert!(e.contains("GMPLUS_SF2"));
    }

    fn src(x: Vec<f32>, step: f64, looped: Option<(usize, usize)>, gain: f64) -> Source {
        Source { x, rate: 32000.0, step, looped, speed: 1.0, env: ModEnv::default(), pitch_env: 0.0, filter: None, gain, pan: 0.0 }
    }

    /// Seam check: the wrap from the loop end to its start is no bigger than a typical sample step.
    fn seam_ok(y: &[f32], a: usize, l: usize) -> bool {
        let jump = (y[a + l - 1] - y[a]).abs();
        let typical = (1..l).map(|k| (y[a + k] - y[a + k - 1]).abs()).fold(0.0f32, f32::max);
        jump <= typical * 1.5 + 1e-3
    }

    /// Review C2: an octave layer locks to the primary's loop and is summed with its gain.
    #[test]
    fn octave_layer_locks_and_sums() {
        let a = src(saw(80.0, 6000), 1.0, Some((1600, 800)), 1.0);
        let b = src(saw(40.0, 6000), 1.0, Some((1600, 400)), 0.5);
        let (y, lp, dev, worst, _, xf) = render(&[a, b], 0);
        let (s, l) = lp.unwrap();
        assert_eq!(xf, 0);
        assert!(dev.abs() < 1e-6 && worst < 1e-6, "{dev} {worst}");
        assert!(seam_ok(&y, s as usize, l as usize));
        // The sum is louder than the primary alone.
        let (y1, ..) = render(&[src(saw(80.0, 6000), 1.0, Some((1600, 800)), 1.0)], 0);
        let rms = |v: &[f32]| (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt();
        assert!(rms(&y[..2000]) > rms(&y1[..2000]) * 1.05);
    }

    /// Review C2: a detuned unison layer that cannot lock keeps its pitch and is crossfaded; the loop still wraps.
    #[test]
    fn detuned_layer_crossfades() {
        let a = src(saw(100.0, 8000), 1.0, Some((2000, 1000)), 1.0);
        let b = src(saw(100.0, 8000), 1.0 * (7.0f64 / 1200.0).exp2(), Some((2000, 1000)), 1.0);
        let (y, lp, dev, _, _, xf) = render(&[a, b], 0);
        let (s, l) = lp.unwrap();
        assert_eq!(xf, 1);
        assert!(dev.abs() <= 0.5);
        assert!(seam_ok(&y, s as usize, l as usize), "seam");
        // Entering the loop is continuous too.
        assert!(seam_ok(&y, s as usize - 1, 2));
    }

    /// Review C3: at a large read step the kept loop is fully read from periodic source (no zero-padded dip).
    #[test]
    fn high_step_loop_keeps_its_level() {
        let x = saw(20.0, 40000);
        let (y, lp, ..) = render(&[src(x, 17.9, Some((20000, 2000)), 1.0)], 0);
        let (a, l) = lp.unwrap();
        let (a, l) = (a as usize, l as usize);
        let rms = |v: &[f32]| (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt();
        let (head, tail) = (rms(&y[a..a + l / 4]), rms(&y[a + 3 * l / 4..a + l]));
        assert!((head / tail - 1.0).abs() < 0.05, "loop level {head} vs {tail}");
    }

    /// Review M7: a looped source with DC gets its loop mean removed, without a high-pass start-up thump in the loop.
    #[test]
    fn looped_dc_is_the_loop_mean() {
        let x: Vec<f32> = saw(50.0, 3000).into_iter().map(|v| v + 0.2).collect();
        let s = src(x, 1.0, Some((100, 500)), 1.0);
        let d = s.x_dc();
        let mean = d[100..600].iter().sum::<f32>() / 500.0;
        assert!(mean.abs() < 1e-4, "{mean}");
    }

    /// Task 4.2: two zones panned apart give differing left and right channels of equal length and loop; a centred
    /// zone sits at −3 dB on both sides.
    #[test]
    fn panned_zones_make_two_channels() {
        let mut a = src(saw(80.0, 6000), 1.0, Some((1600, 800)), 1.0);
        let mut b = src(saw(40.0, 6000), 1.0, Some((1600, 400)), 1.0);
        (a.pan, b.pan) = (-300.0, 300.0);
        let zs = [a, b];
        let (l, ll, ..) = render(&side(&zs, false), 0);
        let (r, lr, ..) = render(&side(&zs, true), 0);
        assert_eq!((l.len(), ll), (r.len(), lr));
        let d: f32 = l.iter().zip(&r).map(|(x, y)| (x - y).abs()).sum();
        assert!(d > 1.0, "channels equal");
        let (pl, pr) = pan_gains(0.0);
        assert!((pl - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-9 && (pr - pl).abs() < 1e-9);
        assert!(pan_gains(-500.0).1.abs() < 1e-9);
    }
}
