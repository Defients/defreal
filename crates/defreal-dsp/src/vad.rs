//! Lightweight speech-activity estimation.
//!
//! Not a recognizer: an envelope follower with hysteresis + hangover.
//! Used so the motion engine prefers to move during pauses/phrases rather
//! than mid-phoneme, and to scale movement expressiveness.

use crate::envelope::EnvelopeFollower;

#[derive(Clone, Copy, Debug)]
pub struct SpeechGate {
    env: EnvelopeFollower,
    open_db: f32,
    close_db: f32,
    hangover_samples: u32,
    hangover: u32,
    open: bool,
}

impl SpeechGate {
    /// open at `open_db`, stay open until level falls below `close_db`
    /// for `hangover_ms`.
    pub fn new(sr: f32, open_db: f32, close_db: f32, hangover_ms: f32) -> Self {
        Self {
            env: EnvelopeFollower::new(8.0, 60.0, sr),
            open_db,
            close_db,
            hangover_samples: (hangover_ms * 0.001 * sr) as u32,
            hangover: 0,
            open: false,
        }
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> bool {
        let e = self.env.process(x);
        let db = crate::envelope::to_db(e);
        if self.open {
            if db < self.close_db {
                if self.hangover > 0 {
                    self.hangover -= 1;
                } else {
                    self.open = false;
                }
            } else {
                self.hangover = self.hangover_samples;
            }
        } else if db > self.open_db {
            self.open = true;
            self.hangover = self.hangover_samples;
        }
        self.open
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Smoothed signal envelope (linear) — used as "loudness" hint.
    pub fn level(&self) -> f32 {
        self.env.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    #[test]
    fn opens_on_speech_closes_on_silence() {
        let sr = 48000.0;
        let mut g = SpeechGate::new(sr, -45.0, -55.0, 200.0);
        // silence
        for _ in 0..4800 {
            assert!(!g.process(0.0));
        }
        // loud burst
        let mut opened = false;
        for i in 0..9600 {
            let x = 0.2 * (2.0 * PI * 200.0 * i as f32 / sr).sin();
            if g.process(x) {
                opened = true;
            }
        }
        assert!(opened);
        // silence again - hangover keeps it open briefly then closes
        let mut closed_at = None;
        for i in 0..48000 {
            if !g.process(0.0) {
                closed_at = Some(i);
                break;
            }
        }
        assert!(closed_at.is_some());
        assert!(closed_at.unwrap() > 4800, "closed too fast");
    }
}
