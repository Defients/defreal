//! Procedural noise textures for ambience: white, pink, brown, hum.
//! All seeded & deterministic; no recorded assets required.

use crate::rng::Rng;

/// Paul Kellet-style pink noise (3-pole approximation). ~-3 dB/oct.
#[derive(Clone, Copy, Debug)]
pub struct PinkNoise {
    b0: f32,
    b1: f32,
    b2: f32,
}

impl PinkNoise {
    pub fn new() -> Self {
        Self { b0: 0.0, b1: 0.0, b2: 0.0 }
    }

    #[inline]
    pub fn next(&mut self, white: f32) -> f32 {
        self.b0 = 0.99765 * self.b0 + white * 0.0990460;
        self.b1 = 0.96300 * self.b1 + white * 0.2965164;
        self.b2 = 0.57000 * self.b2 + white * 1.0526913;
        (self.b0 + self.b1 + self.b2 + white * 0.1848) * 0.05
    }

    pub fn reset(&mut self) {
        self.b0 = 0.0;
        self.b1 = 0.0;
        self.b2 = 0.0;
    }
}

/// Leaky integrator brown noise. ~-6 dB/oct, heavily low-weighted.
#[derive(Clone, Copy, Debug)]
pub struct BrownNoise {
    acc: f32,
}

impl BrownNoise {
    pub fn new() -> Self {
        Self { acc: 0.0 }
    }

    #[inline]
    pub fn next(&mut self, white: f32) -> f32 {
        self.acc = self.acc * 0.998 + white * 0.02;
        self.acc * 2.0 // modest gain; heavily low-weighted
    }

    pub fn reset(&mut self) {
        self.acc = 0.0;
    }
}

/// Mains-style hum: fundamental + weighted harmonics, slight wobble.
#[derive(Clone, Debug)]
pub struct Hum {
    freq: f32,
    phase: f32,
    sr: f32,
    rng: Rng,
    wobble: f32,
}

impl Hum {
    pub fn new(freq: f32, sr: f32, seed: u64) -> Self {
        Self {
            freq,
            phase: 0.0,
            sr,
            rng: Rng::new(seed ^ 0x5eed_u64),
            wobble: 0.0,
        }
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        // very slow amplitude wobble so hum isn't a sterile sine
        if (self.phase / self.freq) as u32 != ((self.phase - 1.0) / self.freq) as u32 {
            self.wobble += (self.rng.f32() - 0.5) * 0.002;
            self.wobble = self.wobble.clamp(-0.05, 0.05);
        }
        self.phase += 1.0;
        let t = self.phase / self.sr;
        let f = self.freq;
        let s = (2.0 * std::f32::consts::PI * f * t).sin() * 0.6
            + (4.0 * std::f32::consts::PI * f * t).sin() * 0.25
            + (6.0 * std::f32::consts::PI * f * t).sin() * 0.1
            + (8.0 * std::f32::consts::PI * f * t).sin() * 0.05;
        s * (1.0 + self.wobble)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Rng;

    #[test]
    fn pink_noise_bounded_and_deterministic() {
        let mut rng1 = Rng::new(1234);
        let mut rng2 = Rng::new(1234);
        let mut p1 = PinkNoise::new();
        let mut p2 = PinkNoise::new();
        for _ in 0..48000 {
            let w1 = rng1.gauss();
            let w2 = rng2.gauss();
            let a = p1.next(w1);
            let b = p2.next(w2);
            assert_eq!(a, b);
            assert!(a.abs() < 1.5, "pink out of range {a}");
            assert!(a.is_finite());
        }
    }

    #[test]
    fn brown_noise_bounded() {
        let mut rng = Rng::new(9);
        let mut b = BrownNoise::new();
        let mut max_v = 0.0f32;
        for _ in 0..48000 {
            let v = b.next(rng.gauss());
            max_v = max_v.max(v.abs());
        }
        assert!(max_v < 2.5 && max_v > 0.05, "max {max_v}");
    }
}
