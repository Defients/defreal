//! HUMANIZE — recording-chain dynamics.
//!
//! Emulates the gentle imperfections of a real recording path: a slow
//! compressor riding the level, an optional expander for quiet parts, very
//! subtle gain wander (mic technique), optional mild saturation.
//! Conservative by design - it preserves the performance, it doesn't
//! manufacture one.

use crate::dynamics::{Compressor, Expander};
use crate::rng::Rng;
use crate::saturate::soft_clip;
use crate::smooth::SmoothedParam;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct HumanizeParams {
    pub enabled: bool,
    /// compressor drive 0..1
    pub compression: f32,
    /// attack/release ms
    pub attack_ms: f32,
    pub release_ms: f32,
    /// expander enabled + strength 0..1
    pub expander: f32,
    /// slow level wander depth 0..1 (mic-technique drift)
    pub wander: f32,
    /// mild saturation 0..1
    pub saturation: f32,
}

impl Default for HumanizeParams {
    fn default() -> Self {
        Self {
            enabled: true,
            compression: 0.35,
            attack_ms: 12.0,
            release_ms: 220.0,
            expander: 0.0,
            wander: 0.25,
            saturation: 0.05,
        }
    }
}

pub struct Humanize {
    comp: Compressor,
    exp: Expander,
    wander_rng: Rng,
    wander_state: f32,
    wander_target: f32,
    wander_depth: SmoothedParam,
    sat_drive: SmoothedParam,
    comp_mix: SmoothedParam,
    exp_on: SmoothedParam,
    wander_samples: u32,
    sr: f32,
}

impl Humanize {
    pub fn new(sr: f32, seed: u64) -> Self {
        Self {
            comp: Compressor::new(sr),
            exp: Expander::new(sr),
            wander_rng: Rng::new(seed ^ 0x1DEA),
            wander_state: 0.0,
            wander_target: 0.0,
            wander_depth: SmoothedParam::new(0.0, 60.0, sr),
            sat_drive: SmoothedParam::new(1.0, 30.0, sr),
            comp_mix: SmoothedParam::new(0.0, 60.0, sr),
            exp_on: SmoothedParam::new(0.0, 60.0, sr),
            wander_samples: 0,
            sr,
        }
    }

    pub fn update(&mut self, p: &HumanizeParams) {
        let c = p.compression.clamp(0.0, 1.0);
        self.comp.threshold_db = -22.0 + c * 6.0;
        self.comp.ratio = 1.15 + c * 1.85;
        self.comp.knee_db = 9.0;
        self.comp.makeup_db = c * 2.0;
        self.comp.set_times(p.attack_ms.clamp(1.0, 200.0), p.release_ms.clamp(20.0, 2000.0));
        self.comp_mix.set(c);

        let e = p.expander.clamp(0.0, 1.0);
        self.exp.threshold_db = -60.0 + e * 18.0;
        self.exp.ratio = 1.2 + e * 1.0;
        self.exp.range_db = -4.0 - e * 10.0;
        self.exp_on.set(e);

        self.wander_depth.set(p.wander.clamp(0.0, 1.0) * 0.06); // max ~±0.6dB wander
        self.sat_drive.set(1.0 + p.saturation.clamp(0.0, 1.0) * 1.8);
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        // slow wander: random-walk gain offset, re-aimed ~every 2s
        if self.wander_samples == 0 {
            self.wander_target = (self.wander_rng.f32() - 0.5) * 2.0;
            self.wander_samples = (self.sr * 2.0) as u32;
        }
        self.wander_samples -= 1;
        self.wander_state += (self.wander_target - self.wander_state) * 0.00008;
        let wander_g = 1.0 + self.wander_state * self.wander_depth.next();

        let mut y = x * wander_g;

        let cm = self.comp_mix.next();
        if cm > 0.005 {
            let (c, _) = self.comp.process(y);
            y = y * (1.0 - cm * 0.7) + c * (cm * 0.7);
        }

        let ex = self.exp_on.next();
        if ex > 0.01 {
            y = self.exp.process(y);
        }

        let d = self.sat_drive.next();
        if d > 1.01 {
            y = soft_clip(y, d);
        }
        y
    }

    pub fn current_gain_db(&self) -> f32 {
        self.comp.current_gain_db()
    }

    pub fn reset(&mut self) {
        self.comp.reset();
        self.exp.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt()
    }

    #[test]
    fn compresses_hot_signal_gently() {
        let sr = 48000.0;
        let mut h = Humanize::new(sr, 1);
        h.update(&HumanizeParams { compression: 0.8, wander: 0.0, saturation: 0.0, ..Default::default() });
        let n = 9600;
        let mut out = vec![0.0; n];
        for i in 0..n {
            let x = 0.5 * (2.0 * PI * 300.0 * i as f32 / sr).sin();
            out[i] = h.process(x);
        }
        let tin = rms(&vec![0.5; 1]);
        let tout = rms(&out[6000..]);
        assert!(tout < 0.5 * 0.95, "tout {tout} tin {tin}");
        assert!(tout > 0.1);
    }

    #[test]
    fn silence_stays_silent() {
        let mut h = Humanize::new(48000.0, 7);
        h.update(&HumanizeParams::default());
        for _ in 0..4096 {
            assert_eq!(h.process(0.0), 0.0);
        }
    }

    #[test]
    fn wander_is_bounded() {
        let mut h = Humanize::new(48000.0, 3);
        h.update(&HumanizeParams { compression: 0.0, wander: 1.0, saturation: 0.0, expander: 0.0, ..Default::default() });
        let mut min_g = f32::MAX;
        let mut max_g = f32::MIN;
        for _ in 0..96000 {
            let y = h.process(0.0) ;
            let _ = y;
            // wander shows in gain of non-zero input; check via ratio on pulses
        }
        // send a DC-ish pulse and measure
        for _ in 0..48000 {
            let y = h.process(0.2);
            let g = y / 0.2;
            min_g = min_g.min(g);
            max_g = max_g.max(g);
        }
        assert!(min_g > 0.9 && max_g < 1.1, "min {min_g} max {max_g}");
    }
}
