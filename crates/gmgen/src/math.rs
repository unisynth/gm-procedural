//! Deterministic math: only IEEE basic ops (+ − × ÷), no platform libm, no fused multiply-add, so native and wasm32
//! builds produce identical bits. Accuracy is audio-grade (≈ 1e-6 relative), not correctly rounded.

/// `floor` for |x| < 2^31.
#[inline]
pub fn floor(x: f32) -> f32 {
    let t = x as i32 as f32;
    if t > x { t - 1.0 } else { t }
}

/// Fractional part in [0, 1).
#[inline]
pub fn fract(x: f32) -> f32 {
    x - floor(x)
}

/// sin(2π·x), x in cycles (any range).
#[inline]
pub fn sin_cycles(x: f32) -> f32 {
    // Reduce to [-0.5, 0.5), then to [-0.25, 0.25] by symmetry; Taylor to z^11 in z = 2πx (|err| < 1e-6).
    let mut t = x - floor(x + 0.5);
    if t > 0.25 {
        t = 0.5 - t;
    } else if t < -0.25 {
        t = -0.5 - t;
    }
    let z = t * core::f32::consts::TAU;
    let z2 = z * z;
    z * (1.0
        + z2 * (-1.0 / 6.0 + z2 * (1.0 / 120.0 + z2 * (-1.0 / 5040.0 + z2 * (1.0 / 362_880.0 - z2 * (1.0 / 39_916_800.0))))))
}

/// cos(2π·x).
#[inline]
pub fn cos_cycles(x: f32) -> f32 {
    sin_cycles(x + 0.25)
}

/// 2^x, clamped to the f32 range.
pub fn exp2(x: f32) -> f32 {
    if x < -126.0 {
        return 0.0;
    }
    if x > 127.0 {
        return f32::MAX;
    }
    let i = floor(x + 0.5);
    let f = x - i; // [-0.5, 0.5)
    // 2^f = e^(f ln2): Taylor to degree 6 (|rel err| < 2e-7 on this range).
    let g = f * core::f32::consts::LN_2;
    let p = 1.0 + g * (1.0 + g * (0.5 + g * (1.0 / 6.0 + g * (1.0 / 24.0 + g * (1.0 / 120.0 + g * (1.0 / 720.0))))));
    p * f32::from_bits(((i as i32 + 127) as u32) << 23)
}

/// e^x.
pub fn exp(x: f32) -> f32 {
    exp2(x * core::f32::consts::LOG2_E)
}

/// log2(x) for x > 0 (≤ 0 gives -126).
pub fn log2(x: f32) -> f32 {
    if x <= 0.0 {
        return -126.0;
    }
    let b = x.to_bits();
    let e = ((b >> 23) & 0xff) as i32 - 127;
    let m = f32::from_bits((b & 0x007f_ffff) | 0x3f80_0000); // [1, 2)
    // log2(m) via atanh series in s = (m-1)/(m+1).
    let s = (m - 1.0) / (m + 1.0);
    let s2 = s * s;
    let l = 2.0 * s * (1.0 + s2 * (1.0 / 3.0 + s2 * (1.0 / 5.0 + s2 * (1.0 / 7.0 + s2 * (1.0 / 9.0)))));
    e as f32 + l * core::f32::consts::LOG2_E
}

/// dB → linear amplitude.
#[inline]
pub fn db(x: f32) -> f32 {
    exp2(x * (core::f32::consts::LOG2_10 / 20.0))
}

/// Linear amplitude → dB (silence = -2400 dB-ish floor).
pub fn to_db(x: f32) -> f32 {
    log2(x) * (20.0 / core::f32::consts::LOG2_10)
}

/// Square root by Newton iterations from a bit-level guess (deterministic; exact enough for coefficients).
pub fn sqrt(x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    let mut y = f32::from_bits((x.to_bits() >> 1) + 0x1fbd_1df5);
    for _ in 0..4 {
        y = 0.5 * (y + x / y);
    }
    y
}

/// MIDI note → Hz.
#[inline]
pub fn midi_hz(note: f32) -> f32 {
    440.0 * exp2((note - 69.0) / 12.0)
}

/// xorshift32 white noise in [-1, 1).
#[derive(Clone)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9) | 1)
    }
    #[inline]
    pub fn white(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x >> 8) as f32 * (2.0 / 16_777_216.0) - 1.0
    }
}

/// Round half to even and clamp to i16.
pub fn to_i16(x: f32) -> i16 {
    let y = x * 32768.0;
    if y >= 32767.0 {
        return 32767;
    }
    if y <= -32768.0 {
        return -32768;
    }
    let f = floor(y);
    let d = y - f;
    let mut i = f as i32;
    if d > 0.5 || (d == 0.5 && i & 1 != 0) {
        i += 1;
    }
    i as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sin_accuracy() {
        let mut worst = 0.0f64;
        for k in -4000..4000 {
            let x = k as f32 / 997.0;
            let e = (sin_cycles(x) as f64 - (x as f64 * core::f64::consts::TAU).sin()).abs();
            worst = worst.max(e);
        }
        assert!(worst < 2e-6, "sin err {worst}");
    }

    #[test]
    fn exp2_log2_accuracy() {
        for k in -200..200 {
            let x = k as f32 / 13.0;
            let r = exp2(x) as f64 / (2f64).powf(x as f64);
            assert!((r - 1.0).abs() < 1e-6, "exp2({x}) rel {r}");
            let y = exp2(x);
            assert!((log2(y) - x).abs() < 2e-5, "log2(exp2({x})) = {}", log2(y));
        }
        assert!((db(-6.0) - 0.501_187_2).abs() < 1e-6);
        assert!((to_db(0.5) + 6.020_6).abs() < 1e-4);
    }

    #[test]
    fn sqrt_floor_round() {
        for k in 1..1000 {
            let x = k as f32 * 0.37;
            assert!((sqrt(x) as f64 - (x as f64).sqrt()).abs() < 1e-5 * (x as f64).sqrt());
        }
        assert_eq!((floor(-1.5), floor(2.0), floor(2.7)), (-2.0, 2.0, 2.0));
        assert_eq!((to_i16(0.5 / 32768.0), to_i16(1.5 / 32768.0), to_i16(-0.5 / 32768.0)), (0, 2, 0));
        assert_eq!((to_i16(1.0), to_i16(-1.5)), (32767, -32768));
    }

    #[test]
    fn rng_is_repeatable_and_bounded() {
        let (mut a, mut b) = (Rng::new(7), Rng::new(7));
        for _ in 0..10000 {
            let x = a.white();
            assert_eq!(x, b.white());
            assert!((-1.0..1.0).contains(&x));
        }
    }
}
