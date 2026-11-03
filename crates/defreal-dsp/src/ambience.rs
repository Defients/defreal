//! AMBIENCE — continuous procedural environmental texture.
//!
//! Never loops a recording; everything is generated. Independent L/R noise
//! generators with shared profile give subtle width. Runs regardless of
//! speech activity - that's the point of a noise floor.

use crate::biquad::{Biquad, BiquadKind};
use crate::envelope::from_db;
use crate::noise::{BrownNoise, Hum, PinkNoise};
use crate::rng::Rng;
use crate::smooth::SmoothedParam;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AmbienceTexture {
    None,
    RoomTone,
    ComputerFan,
    Ventilation,
    ElectricHum,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AmbienceParams {
    pub enabled: bool,
    pub texture: AmbienceTexture,
    /// texture loudness, dBFS-ish (-60 .. -10)
    pub level_db: f32,
    /// spectral tilt: 0 = darker, 1 = brighter
    pub brightness: f32,
    /// seed for reproducible texture
    pub seed: u64,
}

impl Default for AmbienceParams {
    fn default() -> Self {
        Self {
            enabled: true,
            texture: AmbienceTexture::RoomTone,
            level_db: -48.0,
            brightness: 0.5,
            seed: 0xDEF,
        }
    }
}

struct Voice {
    rng: Rng,
    pink: PinkNoise,
    brown: BrownNoise,
    hum: Hum,
    lp: Biquad,
    hp: Biquad,
    /// slow amplitude wander so the floor "breathes" slightly
    wander_rng: Rng,
    wander: f32,
    wander_target: f32,
    wander_count: u32,
}

impl Voice {
    fn new(sr: f32, seed: u64) -> Self {
        Self {
            rng: Rng::new(seed),
            pink: PinkNoise::new(),
            brown: BrownNoise::new(),
            hum: Hum::new(50.0, sr, seed),
            lp: Biquad::new(BiquadKind::LowPass { freq: 8000.0, q: 0.6 }, sr),
            hp: Biquad::new(BiquadKind::HighPass { freq: 40.0, q: 0.7 }, sr),
            wander_rng: Rng::new(seed ^ 0xBEEF),
            wander: 1.0,
            wander_target: 1.0,
            wander_count: 0,
        }
    }

    /// Update slow wander target ~every 0.7s.
    #[inline]
    fn update_wander(&mut self) {
        if self.wander_count == 0 {
            self.wander_target = 0.85 + self.wander_rng.f32() * 0.3;
            self.wander_count = 33600; // 0.7s @48k
        }
        self.wander_count -= 1;
        self.wander += (self.wander_target - self.wander) * 0.0003;
    }
}

pub struct Ambience {
    l: Voice,
    r: Voice,
    gain: SmoothedParam,
    texture: AmbienceTexture,
    sr: f32,
}

impl Ambience {
    pub fn new(sr: f32, seed: u64) -> Self {
        Self {
            l: Voice::new(sr, seed),
            r: Voice::new(sr, seed.wrapping_add(0x9E3779B9)),
            gain: SmoothedParam::new(0.0, 400.0, sr), // slow fade for texture changes
            texture: AmbienceTexture::None,
            sr,
        }
    }

    /// Control-rate update. Texture switches crossfade via the slow gain ramp.
    pub fn update(&mut self, p: &AmbienceParams) {
        if p.texture != self.texture {
            self.texture = p.texture;
            // retint filters for the new texture
            let (hp, lp) = match self.texture {
                AmbienceTexture::None => (40.0, 8000.0),
                AmbienceTexture::RoomTone => (60.0, 4000.0),
                AmbienceTexture::ComputerFan => (120.0, 2500.0),
                AmbienceTexture::Ventilation => (80.0, 1800.0),
                AmbienceTexture::ElectricHum => (30.0, 1000.0),
            };
            let b = p.brightness.clamp(0.0, 1.0);
            let lp = lp * (0.5 + b);
            let hp = hp * (1.5 - b);
            for v in [&mut self.l, &mut self.r] {
                v.lp.set(BiquadKind::LowPass { freq: lp.min(self.sr * 0.45), q: 0.6 }, self.sr);
                v.hp.set(BiquadKind::HighPass { freq: hp.max(20.0), q: 0.7 }, self.sr);
            }
        }
        let active = p.enabled && self.texture != AmbienceTexture::None;
        self.gain.set(if active { from_db(p.level_db.clamp(-80.0, -6.0)) } else { 0.0 });
    }

    /// Render one stereo frame of ambience.
    #[inline]
    pub fn process(&mut self) -> (f32, f32) {
        if self.gain.is_settled() && self.gain.get() <= 0.0 {
            return (0.0, 0.0);
        }
        let g = self.gain.next();
        (self.process_voice(0) * g, self.process_voice(1) * g)
    }

    #[inline]
    fn process_voice(&mut self, ch: usize) -> f32 {
        let v = if ch == 0 { &mut self.l } else { &mut self.r };
        v.update_wander();
        let white = v.rng.gauss() * 0.33;
        let raw = match self.texture {
            AmbienceTexture::None => 0.0,
            AmbienceTexture::RoomTone => v.pink.next(white) * 0.8 + v.brown.next(white) * 0.25,
            AmbienceTexture::ComputerFan => {
                v.brown.next(white) * 0.5 + v.pink.next(white) * 0.5 + white * 0.05
            }
            AmbienceTexture::Ventilation => {
                v.brown.next(white) * 0.9 + v.pink.next(white) * 0.15
            }
            AmbienceTexture::ElectricHum => v.hum.next() * 0.5 + v.pink.next(white) * 0.1,
        };
        let y = v.hp.process(v.lp.process(raw));
        y * v.wander
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_with_seed() {
        let p = AmbienceParams { texture: AmbienceTexture::RoomTone, level_db: -30.0, ..Default::default() };
        let mut a = Ambience::new(48000.0, 99);
        let mut b = Ambience::new(48000.0, 99);
        a.update(&p);
        b.update(&p);
        for _ in 0..48000 {
            a.gain.next(); b.gain.next();
            let x = a.process_voice(0);
            let y = b.process_voice(0);
            assert_eq!(x, y);
        }
    }

    #[test]
    fn bounded_output() {
        let p = AmbienceParams { texture: AmbienceTexture::ComputerFan, level_db: -12.0, ..Default::default() };
        let mut a = Ambience::new(48000.0, 5);
        a.update(&p);
        a.gain.snap(from_db(-12.0));
        let mut max_v = 0.0f32;
        for _ in 0..96000 {
            max_v = max_v.max(a.process_voice(0).abs());
        }
        assert!(max_v < 0.9, "max {max_v}");
    }
}
