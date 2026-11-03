//! Level metering helpers shared between engine and tests.

pub use crate::envelope::{from_db, to_db, LevelMeter};

/// Stereo-aware meter: tracks the max of both channels for peak,
/// and combined power for RMS.
#[derive(Clone, Copy, Debug, Default)]
pub struct StereoMeter {
    pub l: LevelMeter,
    pub r: LevelMeter,
}

impl StereoMeter {
    pub fn reset(&mut self) {
        self.l.reset();
        self.r.reset();
    }

    pub fn push_frame(&mut self, l: f32, r: f32) {
        self.l.push(l);
        self.r.push(r);
    }

    /// (rms, peak) combined
    pub fn snapshot(&mut self, decay: f32) -> (f32, f32) {
        let (rl, pl) = self.l.snapshot(decay);
        let (rr, pr) = self.r.snapshot(decay);
        let rms = ((rl * rl + rr * rr) * 0.5).sqrt();
        (rms, pl.max(pr))
    }
}
