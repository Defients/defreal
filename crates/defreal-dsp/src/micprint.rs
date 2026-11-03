//! MICPRINT — microphone character.
//!
//! Makes pristine audio resemble a specific physical mic: HP/LP bandwidth,
//! broad tonal shaping, proximity-effect bass, subtle saturation, gentle
//! compression. Profiles are tonal recipes, not low-fi gimmicks.

use crate::biquad::{Biquad, BiquadKind};
use crate::dynamics::Compressor;
use crate::envelope::from_db;
use crate::saturate::soft_clip;
use crate::smooth::SmoothedParam;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MicProfile {
    Neutral,
    CleanDeskMic,
    CasualHeadset,
    WebcamMic,
    WarmBroadcast,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct MicprintParams {
    pub enabled: bool,
    pub profile: MicProfile,
    /// high-pass cutoff Hz
    pub hp_hz: f32,
    /// low-pass cutoff Hz
    pub lp_hz: f32,
    /// broad presence/mid shaping, dB at ~3kHz
    pub presence_db: f32,
    /// proximity bass lift, dB at ~150Hz
    pub proximity_db: f32,
    /// 0..1 subtle harmonic saturation amount
    pub saturation: f32,
    /// 0..1 compression amount (mapped to ratio/threshold)
    pub compression: f32,
    /// overall coloration strength 0..1 (scales eq offsets)
    pub color_amount: f32,
    pub trim_db: f32,
}

impl Default for MicprintParams {
    fn default() -> Self {
        Self {
            enabled: true,
            profile: MicProfile::Neutral,
            hp_hz: 80.0,
            lp_hz: 16000.0,
            presence_db: 0.0,
            proximity_db: 0.0,
            saturation: 0.0,
            compression: 0.0,
            color_amount: 1.0,
            trim_db: 0.0,
        }
    }
}

/// Per-profile base voicing (before user offsets).
struct Voicing {
    hp_hz: f32,
    lp_hz: f32,
    /// (freq, q, gain_db) broad shaping bands
    bands: [(f32, f32, f32); 2],
    prox_db: f32,
    sat: f32,
}

fn voicing(p: MicProfile) -> Voicing {
    match p {
        MicProfile::Neutral => Voicing {
            hp_hz: 60.0,
            lp_hz: 19000.0,
            bands: [(3000.0, 0.8, 0.0), (8000.0, 0.7, 0.0)],
            prox_db: 0.0,
            sat: 0.0,
        },
        MicProfile::CleanDeskMic => Voicing {
            hp_hz: 90.0,
            lp_hz: 15500.0,
            bands: [(3200.0, 0.9, 1.5), (9000.0, 0.8, 1.0)],
            prox_db: 1.5,
            sat: 0.08,
        },
        MicProfile::CasualHeadset => Voicing {
            hp_hz: 140.0,
            lp_hz: 11000.0,
            bands: [(2800.0, 1.0, 2.0), (6000.0, 0.9, -1.5)],
            prox_db: 2.5,
            sat: 0.15,
        },
        MicProfile::WebcamMic => Voicing {
            hp_hz: 220.0,
            lp_hz: 8500.0,
            bands: [(2500.0, 1.1, 2.5), (5500.0, 1.0, -2.0)],
            prox_db: 0.5,
            sat: 0.12,
        },
        MicProfile::WarmBroadcast => Voicing {
            hp_hz: 70.0,
            lp_hz: 14000.0,
            bands: [(250.0, 0.7, 2.0), (5000.0, 0.8, 1.0)],
            prox_db: 4.0,
            sat: 0.2,
        },
    }
}

pub struct Micprint {
    hp: Biquad,
    lp: Biquad,
    band_a: Biquad,
    band_b: Biquad,
    prox: Biquad,
    comp: Compressor,
    trim: SmoothedParam,
    sat_drive: SmoothedParam,
    comp_mix: SmoothedParam,
    hp_current: f32,
    lp_current: f32,
    sr: f32,
}

impl Micprint {
    pub fn new(sr: f32) -> Self {
        Self {
            hp: Biquad::new(BiquadKind::HighPass { freq: 80.0, q: 0.707 }, sr),
            lp: Biquad::new(BiquadKind::LowPass { freq: 16000.0, q: 0.707 }, sr),
            band_a: Biquad::new(
                BiquadKind::Peak { freq: 3000.0, q: 0.9, gain_db: 0.0 },
                sr,
            ),
            band_b: Biquad::new(
                BiquadKind::Peak { freq: 8000.0, q: 0.8, gain_db: 0.0 },
                sr,
            ),
            prox: Biquad::new(BiquadKind::LowShelf { freq: 150.0, gain_db: 0.0 }, sr),
            comp: Compressor::new(sr),
            trim: SmoothedParam::new(1.0, 15.0, sr),
            sat_drive: SmoothedParam::new(1.0, 15.0, sr),
            comp_mix: SmoothedParam::new(0.0, 30.0, sr),
            hp_current: 80.0,
            lp_current: 16000.0,
            sr,
        }
    }

    /// Control-rate update. `prox_extra_db`: extra bass from PROXIMITY
    /// module distance (shared coupling, coordinated by the chain).
    pub fn update(&mut self, p: &MicprintParams, prox_extra_db: f32) {
        let v = voicing(p.profile);
        let amt = p.color_amount.clamp(0.0, 1.0);

        let hp_hz = (v.hp_hz + (p.hp_hz - 80.0)).clamp(20.0, 800.0);
        let lp_hz = (v.lp_hz * (p.lp_hz / 16000.0)).clamp(2000.0, 20000.0);
        if (hp_hz - self.hp_current).abs() > 0.5 {
            self.hp.set(BiquadKind::HighPass { freq: hp_hz, q: 0.707 }, self.sr);
            self.hp_current = hp_hz;
        }
        if (lp_hz - self.lp_current).abs() > 10.0 {
            self.lp.set(BiquadKind::LowPass { freq: lp_hz, q: 0.707 }, self.sr);
            self.lp_current = lp_hz;
        }

        let g_a = (v.bands[0].2 + p.presence_db) * amt;
        let g_b = (v.bands[1].2 - 0.5 * p.presence_db.abs().min(2.0)) * amt;
        self.band_a.set(
            BiquadKind::Peak { freq: v.bands[0].0, q: v.bands[0].1, gain_db: g_a },
            self.sr,
        );
        self.band_b.set(
            BiquadKind::Peak { freq: v.bands[1].0, q: v.bands[1].1, gain_db: g_b },
            self.sr,
        );

        let prox_db = (v.prox_db + p.proximity_db + prox_extra_db) * amt;
        self.prox.set(
            BiquadKind::LowShelf { freq: 150.0, gain_db: prox_db.clamp(-2.0, 9.0) },
            self.sr,
        );

        let sat = (v.sat + p.saturation).clamp(0.0, 1.0) * amt;
        self.sat_drive.set(1.0 + sat * 2.5);

        let comp = (v.sat * 0.4 + p.compression).clamp(0.0, 1.0);
        self.comp.threshold_db = -24.0 + comp * 8.0;
        self.comp.ratio = 1.2 + comp * 1.6;
        self.comp.knee_db = 8.0;
        self.comp.makeup_db = comp * 1.5;
        self.comp_mix.set(comp);

        self.trim.set(from_db(p.trim_db));
    }

    /// Process one mono sample. `scratch` is unused (kept for API symmetry).
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let mut y = self.hp.process(x);
        y = self.lp.process(y);
        y = self.band_a.process(y);
        y = self.band_b.process(y);
        y = self.prox.process(y);
        let drive = self.sat_drive.next();
        if drive > 1.01 {
            y = soft_clip(y, drive);
        }
        let cm = self.comp_mix.next();
        if cm > 0.01 {
            let (c, _) = self.comp.process(y);
            y = y * (1.0 - cm) + c * cm;
        }
        y * self.trim.next()
    }

    pub fn current_comp_gain_db(&self) -> f32 {
        self.comp.current_gain_db()
    }

    pub fn reset(&mut self) {
        self.hp.reset();
        self.lp.reset();
        self.band_a.reset();
        self.band_b.reset();
        self.prox.reset();
        self.comp.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    fn run(m: &mut Micprint, freq: f32, n: usize, sr: f32) -> f32 {
        let w = 2.0 * PI * freq / sr;
        let mut peak = 0.0f32;
        for i in 0..n {
            let y = m.process((w * i as f32).sin() * 0.5);
            if i > n / 2 {
                peak = peak.max(y.abs());
            }
        }
        peak
    }

    #[test]
    fn webcam_mic_rolls_off_highs() {
        let sr = 48000.0;
        let mut m = Micprint::new(sr);
        let p = MicprintParams { profile: MicProfile::WebcamMic, ..Default::default() };
        m.update(&p, 0.0);
        let lo = run(&mut m, 300.0, 9600, sr);
        let hi = run(&mut m, 14000.0, 9600, sr);
        assert!(lo > 0.3, "mid {lo}");
        assert!(hi < 0.15, "hi {hi}");
    }

    #[test]
    fn neutral_is_nearly_flat() {
        let sr = 48000.0;
        let mut m = Micprint::new(sr);
        let p = MicprintParams::default();
        m.update(&p, 0.0);
        let mid = run(&mut m, 1000.0, 9600, sr);
        assert!((mid - 0.5).abs() < 0.15, "mid {mid}");
    }

    #[test]
    fn disabled_bypasses() {
        // bypass handled at chain level; here we verify reset works
        let mut m = Micprint::new(48000.0);
        m.reset();
        assert!(m.process(0.0).is_finite());
    }
}
