//! RBJ audio-eq-cookbook biquad filters. Direct Form II transposed.
//! Coefficients are recomputed at control rate; state persists for
//! click-free updates.

use std::f32::consts::PI;

#[derive(Clone, Copy, Debug)]
pub enum BiquadKind {
    LowPass { freq: f32, q: f32 },
    HighPass { freq: f32, q: f32 },
    Peak { freq: f32, q: f32, gain_db: f32 },
    LowShelf { freq: f32, gain_db: f32 },
    HighShelf { freq: f32, gain_db: f32 },
    Notch { freq: f32, q: f32 },
}

#[derive(Clone, Copy, Debug)]
pub struct BiquadCoeffs {
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    pub a1: f32,
    pub a2: f32,
}

impl BiquadCoeffs {
    pub fn design(kind: BiquadKind, sr: f32) -> Self {
        let (freq, q) = match kind {
            BiquadKind::LowPass { freq, q }
            | BiquadKind::HighPass { freq, q }
            | BiquadKind::Peak { freq, q, .. }
            | BiquadKind::Notch { freq, q } => (freq, q),
            BiquadKind::LowShelf { freq, .. } | BiquadKind::HighShelf { freq, .. } => {
                (freq, 0.7071)
            }
        };
        let freq = freq.clamp(10.0, sr * 0.45);
        let w0 = 2.0 * PI * freq / sr;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * q.max(0.05));

        let (b0, b1, b2, a0, a1, a2) = match kind {
            BiquadKind::LowPass { .. } => {
                let b0 = (1.0 - cos_w0) * 0.5;
                let b1 = 1.0 - cos_w0;
                let b2 = b0;
                (b0, b1, b2, 1.0 + alpha, -2.0 * cos_w0, 1.0 - alpha)
            }
            BiquadKind::HighPass { .. } => {
                let b0 = (1.0 + cos_w0) * 0.5;
                let b1 = -(1.0 + cos_w0);
                let b2 = b0;
                (b0, b1, b2, 1.0 + alpha, -2.0 * cos_w0, 1.0 - alpha)
            }
            BiquadKind::Peak { gain_db, .. } => {
                let a = 10f32.powf(gain_db / 40.0);
                (
                    1.0 + alpha * a,
                    -2.0 * cos_w0,
                    1.0 - alpha * a,
                    1.0 + alpha / a,
                    -2.0 * cos_w0,
                    1.0 - alpha / a,
                )
            }
            BiquadKind::Notch { .. } => {
                (1.0, -2.0 * cos_w0, 1.0, 1.0 + alpha, -2.0 * cos_w0, 1.0 - alpha)
            }
            BiquadKind::LowShelf { gain_db, .. } => {
                let a = 10f32.powf(gain_db / 40.0);
                let sa = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) - (a - 1.0) * cos_w0 + sa),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w0),
                    a * ((a + 1.0) - (a - 1.0) * cos_w0 - sa),
                    (a + 1.0) + (a - 1.0) * cos_w0 + sa,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cos_w0),
                    (a + 1.0) + (a - 1.0) * cos_w0 - sa,
                )
            }
            BiquadKind::HighShelf { gain_db, .. } => {
                let a = 10f32.powf(gain_db / 40.0);
                let sa = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) + (a - 1.0) * cos_w0 + sa),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0),
                    a * ((a + 1.0) + (a - 1.0) * cos_w0 - sa),
                    (a + 1.0) - (a - 1.0) * cos_w0 + sa,
                    2.0 * ((a - 1.0) + (a + 1.0) * cos_w0),
                    (a + 1.0) - (a - 1.0) * cos_w0 - sa,
                )
            }
        };

        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    /// Poles of the filter; used by tests for stability validation.
    pub fn poles(&self) -> Option<(f32, f32)> {
        // roots of 1 + a1 z^-1 + a2 z^-2 = 0  =>  z^2 + a1 z + a2 = 0
        let disc = self.a1 * self.a1 - 4.0 * self.a2;
        if disc >= 0.0 {
            let r1 = (-self.a1 + disc.sqrt()) * 0.5;
            let r2 = (-self.a1 - disc.sqrt()) * 0.5;
            Some((r1.abs(), r2.abs()))
        } else {
            let mag = self.a2.abs().sqrt();
            Some((mag, mag))
        }
    }

    pub fn is_stable(&self) -> bool {
        match self.poles() {
            Some((p1, p2)) => p1 < 1.0 && p2 < 1.0,
            None => true,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Biquad {
    c: BiquadCoeffs,
    z1: f32,
    z2: f32,
}

impl Biquad {
    pub fn new(kind: BiquadKind, sr: f32) -> Self {
        Self {
            c: BiquadCoeffs::design(kind, sr),
            z1: 0.0,
            z2: 0.0,
        }
    }

    /// Update coefficients without clearing state (click-free).
    pub fn set(&mut self, kind: BiquadKind, sr: f32) {
        self.c = BiquadCoeffs::design(kind, sr);
    }

    pub fn coeffs(&self) -> BiquadCoeffs {
        self.c
    }

    #[inline(always)]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.c.b0 * x + self.z1;
        self.z1 = self.c.b1 * x - self.c.a1 * y + self.z2;
        self.z2 = self.c.b2 * x - self.c.a2 * y;
        y
    }

    pub fn process_block(&mut self, buf: &mut [f32]) {
        for s in buf.iter_mut() {
            *s = self.process(*s);
        }
    }

    /// Convenience: run over an immutable input into an output buffer.
    pub fn process_into(&mut self, input: &[f32], out: &mut [f32]) {
        for (i, o) in input.iter().zip(out.iter_mut()) {
            *o = self.process(*i);
        }
    }

    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

/// One-pole lowpass - cheapest possible coloration/damping.
#[derive(Clone, Copy, Debug)]
pub struct OnePole {
    a: f32, // feed-forward coefficient
    z: f32,
}

impl OnePole {
    pub fn lowpass(freq: f32, sr: f32) -> Self {
        let x = (-2.0 * PI * freq / sr).exp();
        Self { a: 1.0 - x, z: 0.0 }
    }

    pub fn set_lowpass(&mut self, freq: f32, sr: f32) {
        self.a = 1.0 - (-2.0 * PI * freq / sr).exp();
    }

    #[inline(always)]
    pub fn process(&mut self, x: f32) -> f32 {
        self.z += self.a * (x - self.z);
        self.z
    }

    pub fn reset(&mut self) {
        self.z = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sr() -> f32 {
        48000.0
    }

    /// Measure amplitude response by feeding a sine and checking steady-state gain.
    fn response_at(kind: BiquadKind, test_freq: f32) -> f32 {
        let sr = sr();
        let mut f = Biquad::new(kind, sr);
        let n = 4800;
        let w = 2.0 * PI * test_freq / sr;
        let mut peak = 0.0f32;
        for i in 0..n {
            let x = (w * i as f32).sin();
            let y = f.process(x);
            if i > n / 2 {
                peak = peak.max(y.abs());
            }
        }
        peak
    }

    #[test]
    fn lowpass_attenuates_highs() {
        let kind = BiquadKind::LowPass { freq: 2000.0, q: 0.707 };
        let lo = response_at(kind, 200.0);
        let hi = response_at(kind, 12000.0);
        assert!(lo > 0.9, "passband gain {lo}");
        assert!(hi < 0.05, "stopband gain {hi}");
    }

    #[test]
    fn highpass_attenuates_lows() {
        let kind = BiquadKind::HighPass { freq: 300.0, q: 0.707 };
        let lo = response_at(kind, 30.0);
        let hi = response_at(kind, 5000.0);
        assert!(hi > 0.9, "passband {hi}");
        assert!(lo < 0.02, "stopband {lo}");
    }

    #[test]
    fn peak_boosts_at_freq() {
        let kind = BiquadKind::Peak { freq: 3000.0, q: 1.0, gain_db: 6.0 };
        let at = response_at(kind, 3000.0);
        let away = response_at(kind, 200.0);
        assert!((at - 2.0).abs() < 0.15, "peak gain {at}");
        assert!((away - 1.0).abs() < 0.05, "off-freq {away}");
    }

    #[test]
    fn coefficients_stable_across_range() {
        for freq in [20.0, 100.0, 500.0, 2000.0, 8000.0, 18000.0] {
            for q in [0.3, 0.707, 1.5, 4.0] {
                let c = BiquadCoeffs::design(BiquadKind::LowPass { freq, q }, sr());
                assert!(c.is_stable(), "LP {freq} {q}");
                let c = BiquadCoeffs::design(BiquadKind::HighPass { freq, q }, sr());
                assert!(c.is_stable(), "HP {freq} {q}");
            }
        }
    }

    #[test]
    fn no_nan_on_impulse() {
        let mut f = Biquad::new(BiquadKind::LowPass { freq: 1000.0, q: 4.0 }, sr());
        for i in 0..1024 {
            let x = if i == 0 { 1.0 } else { 0.0 };
            let y = f.process(x);
            assert!(y.is_finite());
        }
    }
}
