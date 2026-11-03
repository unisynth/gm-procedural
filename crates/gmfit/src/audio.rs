//! Original waves, the "played" unroll (attack + loop repeated, as a sampler plays a held note), WAV output.
use gmgen::render::WaveMeta;

/// Samples of wave `i` of the original bank as f32.
pub fn original_wave(d: &dls::Dls, i: usize) -> Vec<f32> {
    match d.wave(i as u32) {
        Some(w) => (0..w.len()).map(|k| w.sample(k)).collect(),
        None => Vec::new(),
    }
}

/// Usable loop `(start, len)` of a wave.
pub fn loop_of(m: &WaveMeta) -> Option<(usize, usize)> {
    m.looped.map(|(s, l)| (s as usize, l as usize)).filter(|&(s, l)| l > 0 && s + l <= m.len)
}

/// Attack, then the loop repeated until at least `secs` seconds (unlooped waves are returned as they are).
pub fn played(x: &[f32], m: &WaveMeta, secs: f32) -> Vec<f32> {
    let Some((s, l)) = loop_of(m) else { return x.to_vec() };
    if x.len() < s + l {
        return x.to_vec();
    }
    let want = ((secs * m.rate as f32) as usize).max(s + l);
    let mut out = x[..s + l].to_vec();
    while out.len() < want {
        let k = (out.len() - s) % l;
        out.push(x[s + k]);
    }
    out
}

/// Writes mono 16-bit PCM.
pub fn write_wav(path: &std::path::Path, x: &[f32], rate: u32) -> std::io::Result<()> {
    let mut b = Vec::with_capacity(44 + x.len() * 2);
    let data = (x.len() * 2) as u32;
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    for v in [16u32] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    for v in [1u16, 1] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * 2).to_le_bytes());
    for v in [2u16, 16] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    for s in x {
        b.extend_from_slice(&gmgen::math::to_i16(*s).to_le_bytes());
    }
    std::fs::write(path, b)
}
