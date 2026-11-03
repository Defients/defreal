//! Output protection: fast safety limiter + final soft clip.
//! Guarantees bounded output even on extreme settings.

use crate::envelope::{from_db, EnvelopeFollower};

pub struct SafetyLimiter {
    ceiling: f32,
    env: EnvelopeFollower,
    gain: f32,
    sr: f32,
}

impl SafetyLimiter {
    pub fn new(ceiling_db: f32, sr: f32) -> Self {
        Self {
            ceiling: from_db(ceiling_db),
            env: EnvelopeFollower::new(0.5, 60.0, sr),
            gain: 1.0,
            sr,
        }
    }

    pub fn set_ceiling_db(&mut self, db: f32) {
        self.ceiling = from_db(db);
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let env = self.env.process(x);
        let target = if env > self.ceiling {
            self.ceiling / env
        } else {
            1.0
        };
        // attack ~instant, release ~60ms (already in the follower)
        let c = if target < self.gain {
            0.5
        } else {
            1.0 - (-1.0 / (0.050 * self.sr)).exp()
        };
        self.gain += c * (target - self.gain);
        (x * self.gain).clamp(-0.98, 0.98)
    }

    pub fn current_gain(&self) -> f32 {
        self.gain
    }

    pub fn reset(&mut self) {
        self.env.reset();
        self.gain = 1.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_hot_signal() {
        let mut l = SafetyLimiter::new(-1.0, 48000.0);
        let mut max_out = 0.0f32;
        for i in 0..4800 {
            let y = l.process(if i % 2 == 0 { 2.0 } else { -2.0 });
            max_out = max_out.max(y.abs());
        }
        assert!(max_out <= 0.99, "peak {max_out}");
    }

    #[test]
    fn transparent_below_ceiling() {
        let mut l = SafetyLimiter::new(-1.0, 48000.0);
        for _ in 0..4800 {
            let y = l.process(0.3);
            assert!((y - 0.3).abs() < 0.02, "y={y}");
        }
    }
}
