//! Gentle dynamics: soft-knee compressor, downward expander.
//! dB-domain gain computer with attack/release envelope.

use crate::envelope::{from_db, to_db, EnvelopeFollower};

#[derive(Clone, Copy, Debug)]
pub struct Compressor {
    pub threshold_db: f32,
    pub ratio: f32,
    pub knee_db: f32,
    pub makeup_db: f32,
    env: EnvelopeFollower,
    gain: f32, // smoothed gain multiplier
    sr: f32,
    attack_ms: f32,
    release_ms: f32,
}

impl Compressor {
    pub fn new(sr: f32) -> Self {
        Self {
            threshold_db: -18.0,
            ratio: 2.0,
            knee_db: 6.0,
            makeup_db: 0.0,
            env: EnvelopeFollower::new(10.0, 150.0, sr),
            gain: 1.0,
            sr,
            attack_ms: 10.0,
            release_ms: 150.0,
        }
    }

    pub fn set_times(&mut self, attack_ms: f32, release_ms: f32) {
        self.attack_ms = attack_ms;
        self.release_ms = release_ms;
        self.env.set_times(attack_ms, release_ms, self.sr);
    }

    /// Static gain computer: input level dB -> gain dB (negative = reduction).
    pub fn gain_db_for(&self, level_db: f32) -> f32 {
        let t = self.threshold_db;
        let k = self.knee_db.max(0.1);
        let over = level_db - t;
        let compressed = if over <= -k * 0.5 {
            0.0
        } else if over < k * 0.5 {
            // soft knee region
            let x = over + k * 0.5;
            -(1.0 - 1.0 / self.ratio) * x * x / (2.0 * k)
        } else {
            -(1.0 - 1.0 / self.ratio) * over
        };
        compressed + self.makeup_db
    }

    /// Process one sample, returns (output, current gain).
    #[inline]
    pub fn process(&mut self, x: f32) -> (f32, f32) {
        let env = self.env.process(x);
        let level_db = to_db(env);
        let target_db = self.gain_db_for(level_db);
        let target = from_db(target_db);
        // gain smoothing reuses the follower ballistics implicitly via env,
        // but smooth the gain itself to be safe at fast settings
        let c = if target < self.gain {
            self.env_attack_coef()
        } else {
            self.env_release_coef()
        };
        self.gain += c * (target - self.gain);
        (x * self.gain, self.gain)
    }

    fn env_attack_coef(&self) -> f32 {
        1.0 - (-1.0 / (self.attack_ms.max(0.05) * 0.001 * self.sr)).exp()
    }
    fn env_release_coef(&self) -> f32 {
        1.0 - (-1.0 / (self.release_ms.max(1.0) * 0.001 * self.sr)).exp()
    }

    pub fn current_gain_db(&self) -> f32 {
        to_db(self.gain)
    }

    pub fn reset(&mut self) {
        self.env.reset();
        self.gain = 1.0;
    }
}

/// Downward expander: reduces gain below a threshold. Used subtly to let
/// room tone rather than gate silence dominate gaps.
#[derive(Clone, Copy, Debug)]
pub struct Expander {
    pub threshold_db: f32,
    pub ratio: f32,
    pub range_db: f32, // max attenuation
    env: EnvelopeFollower,
    gain: f32,
    sr: f32,
    attack_ms: f32,
    release_ms: f32,
}

impl Expander {
    pub fn new(sr: f32) -> Self {
        Self {
            threshold_db: -55.0,
            ratio: 1.6,
            range_db: -12.0,
            env: EnvelopeFollower::new(5.0, 300.0, sr),
            gain: 1.0,
            sr,
            attack_ms: 5.0,
            release_ms: 300.0,
        }
    }

    pub fn gain_db_for(&self, level_db: f32) -> f32 {
        if level_db >= self.threshold_db {
            return 0.0;
        }
        let under = self.threshold_db - level_db;
        ((self.ratio - 1.0) * -under).max(self.range_db)
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let env = self.env.process(x);
        let target = from_db(self.gain_db_for(to_db(env)));
        let c = if target < self.gain {
            1.0 - (-1.0 / (self.attack_ms * 0.001 * self.sr)).exp()
        } else {
            1.0 - (-1.0 / (self.release_ms * 0.001 * self.sr)).exp()
        };
        self.gain += c * (target - self.gain);
        x * self.gain
    }

    pub fn reset(&mut self) {
        self.env.reset();
        self.gain = 1.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    fn sine(n: usize, freq: f32, amp: f32, sr: f32) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * PI * freq * i as f32 / sr).sin())
            .collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt()
    }

    #[test]
    fn compressor_reduces_loud_signal() {
        let sr = 48000.0;
        let mut c = Compressor::new(sr);
        c.threshold_db = -20.0;
        c.ratio = 4.0;
        let input = sine(9600, 440.0, 0.5, sr); // ~ -9dB
        let mut out = vec![0.0; input.len()];
        for (i, &s) in input.iter().enumerate() {
            out[i] = c.process(s).0;
        }
        let tail_in = rms(&input[4800..]);
        let tail_out = rms(&out[4800..]);
        assert!(tail_out < tail_in * 0.75, "in {tail_in} out {tail_out}");
        assert!(tail_out > tail_in * 0.3, "over-compressed {tail_out}");
    }

    #[test]
    fn compressor_passes_quiet_signal() {
        let sr = 48000.0;
        let mut c = Compressor::new(sr);
        c.threshold_db = -20.0;
        c.ratio = 4.0;
        let input = sine(9600, 440.0, 0.02, sr); // ~ -37dB
        let mut out = vec![0.0; input.len()];
        for (i, &s) in input.iter().enumerate() {
            out[i] = c.process(s).0;
        }
        let tail_in = rms(&input[4800..]);
        let tail_out = rms(&out[4800..]);
        assert!((tail_out / tail_in - 1.0).abs() < 0.1);
    }

    #[test]
    fn expander_attenuates_below_threshold() {
        let sr = 48000.0;
        let mut e = Expander::new(sr);
        e.threshold_db = -40.0;
        e.ratio = 2.5;
        e.range_db = -18.0;
        let input = sine(9600, 440.0, 0.005, sr); // ~ -49dB -> -20dB expansion
        let mut out = vec![0.0; input.len()];
        for (i, &s) in input.iter().enumerate() {
            out[i] = e.process(s);
        }
        let tail_out = rms(&out[6000..]);
        let tail_in = rms(&input[6000..]);
        assert!(tail_out < tail_in * 0.5, "in {tail_in} out {tail_out}");
    }

    #[test]
    fn no_nan_on_silence() {
        let mut c = Compressor::new(48000.0);
        for _ in 0..4096 {
            let (y, g) = c.process(0.0);
            assert!(y.is_finite() && g.is_finite());
        }
    }
}
