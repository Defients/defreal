//! Envelope followers and block level measurement.

/// Classic attack/release envelope follower on |x|.
#[derive(Clone, Copy, Debug)]
pub struct EnvelopeFollower {
    attack: f32,
    release: f32,
    env: f32,
}

impl EnvelopeFollower {
    /// attack_ms / release_ms are 10-90% style ballistics.
    pub fn new(attack_ms: f32, release_ms: f32, sr: f32) -> Self {
        Self {
            attack: coef(attack_ms, sr),
            release: coef(release_ms, sr),
            env: 0.0,
        }
    }

    pub fn set_times(&mut self, attack_ms: f32, release_ms: f32, sr: f32) {
        self.attack = coef(attack_ms, sr);
        self.release = coef(release_ms, sr);
    }

    #[inline(always)]
    pub fn process(&mut self, x: f32) -> f32 {
        let a = x.abs();
        let c = if a > self.env { self.attack } else { self.release };
        self.env += c * (a - self.env);
        self.env
    }

    #[inline(always)]
    pub fn get(&self) -> f32 {
        self.env
    }

    pub fn reset(&mut self) {
        self.env = 0.0;
    }
}

#[inline]
fn coef(ms: f32, sr: f32) -> f32 {
    1.0 - (-1.0 / (ms.max(0.01) * 0.001 * sr)).exp()
}

/// RMS + peak accumulated over a block, with peak-hold decay for metering.
#[derive(Clone, Copy, Debug, Default)]
pub struct LevelMeter {
    pub peak: f32,
    pub peak_hold: f32,
    pub sum_sq: f64,
    pub count: u32,
}

impl LevelMeter {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    #[inline]
    pub fn push(&mut self, x: f32) {
        let a = x.abs();
        self.peak = self.peak.max(a);
        self.sum_sq += (a as f64) * (a as f64);
        self.count += 1;
    }

    pub fn push_block(&mut self, buf: &[f32]) {
        for &s in buf {
            self.push(s);
        }
    }

    pub fn rms(&self) -> f32 {
        if self.count == 0 {
            0.0
        } else {
            (self.sum_sq / self.count as f64).sqrt() as f32
        }
    }

    /// Finish a measurement period: returns (rms, peak). Applies decay to
    /// the hold value so the UI sees smooth falloff.
    pub fn snapshot(&mut self, decay: f32) -> (f32, f32) {
        let rms = self.rms();
        self.peak_hold = self.peak.max(self.peak_hold * decay);
        let peak = self.peak_hold;
        self.peak = 0.0;
        self.sum_sq = 0.0;
        self.count = 0;
        (rms, peak)
    }
}

pub fn to_db(linear: f32) -> f32 {
    20.0 * linear.max(1e-8).log10()
}

pub fn from_db(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    #[test]
    fn follows_level_change() {
        let mut e = EnvelopeFollower::new(5.0, 100.0, 48000.0);
        for _ in 0..4800 {
            e.process(0.5);
        }
        assert!(e.get() > 0.4);
        // release tau = 100ms -> need ~3 tau (300ms) to fall below 10%
        for _ in 0..24000 {
            e.process(0.0);
        }
        assert!(e.get() < 0.05);
    }

    #[test]
    fn rms_of_sine() {
        let mut m = LevelMeter::default();
        for i in 0..4800 {
            m.push((2.0 * PI * 440.0 * i as f32 / 48000.0).sin());
        }
        let rms = m.rms();
        assert!((rms - 0.7071).abs() < 0.02, "rms {rms}");
    }

    #[test]
    fn db_conversions_roundtrip() {
        assert!((from_db(to_db(0.5)) - 0.5).abs() < 1e-5);
        assert!((to_db(1.0)).abs() < 1e-6);
        assert!((from_db(-6.0) - 0.501).abs() < 0.005);
    }
}
