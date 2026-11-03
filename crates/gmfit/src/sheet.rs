//! Contact sheets (PNG): per wave, the original and generated waveforms, their log-mel spectrograms (512-point,
//! 64 bands, −80…0 dB) and the difference (red = generated louder, blue = quieter).
use crate::score::mel_spec;
use std::path::Path;

pub const W: usize = 1024;
const WAVE_H: usize = 80;
const SPEC_H: usize = 128;

pub struct Img {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

impl Img {
    pub fn new(w: usize, h: usize) -> Img {
        Img { w, h, px: vec![16; w * h * 3] }
    }
    fn set(&mut self, x: usize, y: usize, c: [u8; 3]) {
        if x < self.w && y < self.h {
            let i = (y * self.w + x) * 3;
            self.px[i..i + 3].copy_from_slice(&c);
        }
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let f = std::fs::File::create(path).map_err(|e| e.to_string())?;
        let mut e = png::Encoder::new(std::io::BufWriter::new(f), self.w as u32, self.h as u32);
        e.set_color(png::ColorType::Rgb);
        e.set_depth(png::BitDepth::Eight);
        e.write_header().and_then(|mut w| w.write_image_data(&self.px)).map_err(|e| e.to_string())
    }
}

fn heat(v: f32) -> [u8; 3] {
    // −80…0 dB → black, blue, magenta, yellow, white.
    let t = ((v + 80.0) / 80.0).clamp(0.0, 1.0);
    let stops = [[0.0, 0.0, 0.0], [0.1, 0.1, 0.6], [0.8, 0.1, 0.5], [1.0, 0.8, 0.1], [1.0, 1.0, 1.0]];
    let p = t * 4.0;
    let i = (p as usize).min(3);
    let f = p - i as f32;
    let c: Vec<u8> = (0..3).map(|k| ((stops[i][k] * (1.0 - f) + stops[i + 1][k] * f) * 255.0) as u8).collect();
    [c[0], c[1], c[2]]
}

fn wave_row(img: &mut Img, y0: usize, x: &[f32], color: [u8; 3]) {
    let mid = y0 + WAVE_H / 2;
    for px in 0..img.w {
        img.set(px, mid, [60, 60, 60]);
        let (a, b) = (px * x.len() / img.w, ((px + 1) * x.len() / img.w).max(px * x.len() / img.w + 1));
        let seg = &x[a.min(x.len())..b.min(x.len())];
        let (lo, hi) = seg.iter().fold((0.0f32, 0.0f32), |(l, h), v| (l.min(*v), h.max(*v)));
        let (y_hi, y_lo) = ((mid as f32 - hi * (WAVE_H / 2) as f32) as usize, (mid as f32 - lo * (WAVE_H / 2) as f32) as usize);
        for y in y_hi..=y_lo.min(y0 + WAVE_H - 1) {
            img.set(px, y, color);
        }
    }
}

fn spec_rows(img: &mut Img, y0: usize, frames: &[Vec<f32>], f: impl Fn(usize, usize) -> [u8; 3]) {
    let bands = frames.first().map_or(1, Vec::len);
    for px in 0..img.w {
        let fi = (px * frames.len() / img.w).min(frames.len().saturating_sub(1));
        for y in 0..SPEC_H {
            let b = ((SPEC_H - 1 - y) * bands / SPEC_H).min(bands - 1);
            img.set(px, y0 + y, f(fi, b));
        }
    }
}

/// Height of one wave block.
pub const BLOCK_H: usize = 2 * WAVE_H + 3 * SPEC_H + 8;

/// Draws original/generated waveforms, both spectrograms and the difference at `y0`.
pub fn draw_pair(img: &mut Img, y0: usize, orig: &[f32], synth: &[f32], rate: f32) {
    wave_row(img, y0, orig, [120, 200, 255]);
    wave_row(img, y0 + WAVE_H, synth, [255, 190, 90]);
    let (fa, _) = mel_spec(orig, rate, 512, 64);
    let (fb, _) = mel_spec(synth, rate, 512, 64);
    let y = y0 + 2 * WAVE_H;
    spec_rows(img, y, &fa, |i, b| heat(fa[i][b]));
    spec_rows(img, y + SPEC_H, &fb, |i, b| heat(fb.get(i).map_or(-80.0, |r| r[b])));
    spec_rows(img, y + 2 * SPEC_H, &fa, |i, b| {
        let d = fb.get(i).map_or(-80.0, |r| r[b]) - fa[i][b];
        let t = (d.abs() / 24.0).clamp(0.0, 1.0);
        let k = (t * 255.0) as u8;
        if d > 0.0 { [k, k / 4, k / 4] } else { [k / 4, k / 4, k] }
    });
}

/// Sheet for one wave.
pub fn pair(path: &Path, orig: &[f32], synth: &[f32], rate: f32) -> Result<(), String> {
    let mut img = Img::new(W, BLOCK_H);
    draw_pair(&mut img, 0, orig, synth, rate);
    img.save(path)
}

/// One spectrogram row per variant (sweeps), original on top.
pub fn rows(path: &Path, orig: &[f32], variants: &[Vec<f32>], rate: f32) -> Result<(), String> {
    let mut img = Img::new(W, (variants.len() + 1) * (SPEC_H + 4));
    let all: Vec<&[f32]> = std::iter::once(orig).chain(variants.iter().map(Vec::as_slice)).collect();
    for (k, x) in all.iter().enumerate() {
        let (f, _) = mel_spec(x, rate, 512, 64);
        spec_rows(&mut img, k * (SPEC_H + 4), &f, |i, b| heat(f[i][b]));
    }
    img.save(path)
}

/// Bank-wide sheet: one cell per wave in the given order (original | generated spectrogram, 64 × 32 each), 8 cells
/// per row, with a score bar under each cell (green A, yellow B, red C).
pub fn bank(path: &Path, cells: &[(usize, f32, Vec<f32>, Vec<f32>)], rate: f32) -> Result<(), String> {
    const CW: usize = 64;
    const CH: usize = 32;
    let cols = 8;
    let (cell_w, cell_h) = (2 * CW + 6, CH + 6);
    let rows = cells.len().div_ceil(cols);
    let mut img = Img::new(cols * cell_w, rows * cell_h);
    for (k, (_, score, a, b)) in cells.iter().enumerate() {
        let (x0, y0) = ((k % cols) * cell_w, (k / cols) * cell_h);
        for (j, x) in [a, b].iter().enumerate() {
            let (f, _) = mel_spec(x, rate, 256, CH);
            for px in 0..CW {
                let fi = (px * f.len() / CW).min(f.len() - 1);
                for y in 0..CH {
                    img.set(x0 + j * (CW + 2) + px, y0 + y, heat(f[fi][CH - 1 - y]));
                }
            }
        }
        let c = if *score >= 0.95 { [40, 200, 60] } else if *score >= 0.85 { [220, 200, 40] } else { [220, 50, 40] };
        for px in 0..((2 * CW + 2) as f32 * score.clamp(0.0, 1.0)) as usize {
            img.set(x0 + px, y0 + CH + 2, c);
            img.set(x0 + px, y0 + CH + 3, c);
        }
    }
    img.save(path)
}
