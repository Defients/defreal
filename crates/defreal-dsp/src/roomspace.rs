//! ROOMSPACE — believable acoustic environment.
//!
//! Early reflections + light FDN tail. Stereo width from slightly detuned
//! L/R delay sets. Profiles describe real rooms, not hall presets.

use crate::reverb::{EarlyReflections, LateReverb, ReflectionTap};
use crate::smooth::SmoothedParam;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RoomProfile {
    Bedroom,
    HomeOffice,
    TreatedStudio,
    SmallLivingRoom,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct RoomspaceParams {
    pub enabled: bool,
    pub room: RoomProfile,
    /// master room level 0..1 (scales ER + tail)
    pub level: f32,
    /// size scale 0.5..1.6 - stretches all delays
    pub size: f32,
    /// decay time scale 0.5..2.0 around profile RT
    pub decay_scale: f32,
    /// early reflection level 0..1
    pub er_level: f32,
    /// late tail level 0..1
    pub tail_level: f32,
    /// damping - high-freq absorbtion Hz
    pub damping_hz: f32,
}

impl Default for RoomspaceParams {
    fn default() -> Self {
        Self {
            enabled: true,
            room: RoomProfile::Bedroom,
            level: 0.5,
            size: 1.0,
            decay_scale: 1.0,
            er_level: 0.7,
            tail_level: 0.5,
            damping_hz: 5200.0,
        }
    }
}

struct RoomSpec {
    er_taps: [ReflectionTap; 6],
    fdn_delays_ms: [f32; 4],
    rt60_ms: f32,
    damping_hz: f32,
    er_gain: f32,
    tail_gain: f32,
}

fn spec(room: RoomProfile) -> RoomSpec {
    match room {
        // small, soft, quick decay, close reflections
        RoomProfile::Bedroom => RoomSpec {
            er_taps: [
                ReflectionTap { delay_ms: 7.0, gain: 0.32 },
                ReflectionTap { delay_ms: 13.0, gain: 0.24 },
                ReflectionTap { delay_ms: 21.0, gain: 0.16 },
                ReflectionTap { delay_ms: 31.0, gain: 0.10 },
                ReflectionTap { delay_ms: 43.0, gain: 0.06 },
                ReflectionTap { delay_ms: 58.0, gain: 0.04 },
            ],
            fdn_delays_ms: [19.7, 24.3, 29.9, 35.1],
            rt60_ms: 240.0,
            damping_hz: 4800.0,
            er_gain: 0.9,
            tail_gain: 0.5,
        },
        // moderate size, brighter
        RoomProfile::HomeOffice => RoomSpec {
            er_taps: [
                ReflectionTap { delay_ms: 9.0, gain: 0.30 },
                ReflectionTap { delay_ms: 17.0, gain: 0.24 },
                ReflectionTap { delay_ms: 27.0, gain: 0.18 },
                ReflectionTap { delay_ms: 39.0, gain: 0.11 },
                ReflectionTap { delay_ms: 55.0, gain: 0.07 },
                ReflectionTap { delay_ms: 72.0, gain: 0.05 },
            ],
            fdn_delays_ms: [23.1, 29.7, 36.3, 43.9],
            rt60_ms: 320.0,
            damping_hz: 6000.0,
            er_gain: 0.85,
            tail_gain: 0.55,
        },
        // short, dead, very few reflections
        RoomProfile::TreatedStudio => RoomSpec {
            er_taps: [
                ReflectionTap { delay_ms: 11.0, gain: 0.14 },
                ReflectionTap { delay_ms: 23.0, gain: 0.09 },
                ReflectionTap { delay_ms: 37.0, gain: 0.05 },
                ReflectionTap { delay_ms: 51.0, gain: 0.03 },
                ReflectionTap { delay_ms: 67.0, gain: 0.02 },
                ReflectionTap { delay_ms: 83.0, gain: 0.01 },
            ],
            fdn_delays_ms: [17.3, 21.9, 26.7, 31.1],
            rt60_ms: 140.0,
            damping_hz: 4000.0,
            er_gain: 0.6,
            tail_gain: 0.35,
        },
        // bigger, longer, more live
        RoomProfile::SmallLivingRoom => RoomSpec {
            er_taps: [
                ReflectionTap { delay_ms: 12.0, gain: 0.34 },
                ReflectionTap { delay_ms: 22.0, gain: 0.27 },
                ReflectionTap { delay_ms: 36.0, gain: 0.20 },
                ReflectionTap { delay_ms: 51.0, gain: 0.14 },
                ReflectionTap { delay_ms: 70.0, gain: 0.09 },
                ReflectionTap { delay_ms: 95.0, gain: 0.06 },
            ],
            fdn_delays_ms: [27.7, 34.9, 43.1, 51.3],
            rt60_ms: 420.0,
            damping_hz: 5500.0,
            er_gain: 1.0,
            tail_gain: 0.7,
        },
    }
}

pub struct Roomspace {
    er_l: EarlyReflections,
    er_r: EarlyReflections,
    late_l: LateReverb,
    late_r: LateReverb,
    send_gain: SmoothedParam,
    wet_gain: SmoothedParam,
    er_gain: SmoothedParam,
    tail_profile_gain: f32,
    size: f32,
    last_spec: Option<RoomProfile>,
}

impl Roomspace {
    pub fn new(sr: f32) -> Self {
        let s = spec(RoomProfile::Bedroom);
        Self {
            er_l: EarlyReflections::new(sr, 120.0),
            er_r: EarlyReflections::new(sr, 120.0),
            // ~7% detune for decorrelation
            late_l: LateReverb::new(sr, s.fdn_delays_ms, s.rt60_ms),
            late_r: {
                let mut d = s.fdn_delays_ms;
                for v in &mut d {
                    *v *= 1.07;
                }
                LateReverb::new(sr, d, s.rt60_ms)
            },
            send_gain: SmoothedParam::new(1.0, 30.0, sr),
            wet_gain: SmoothedParam::new(0.5, 30.0, sr),
            er_gain: SmoothedParam::new(0.7, 30.0, sr),
            tail_profile_gain: 0.5,
            size: 1.0,
            last_spec: None,
        }
    }

    pub fn update(&mut self, p: &RoomspaceParams) {
        // Recompute taps/delays when the room or size changed.
        if self.last_spec != Some(p.room) || (self.size - p.size.clamp(0.5, 1.6)).abs() > 0.01 {
            let s = spec(p.room);
            let size = p.size.clamp(0.5, 1.6);
            // ER taps scaled by size
            let taps: Vec<ReflectionTap> = s
                .er_taps
                .iter()
                .map(|t| ReflectionTap {
                    delay_ms: t.delay_ms * size,
                    gain: t.gain * s.er_gain,
                })
                .collect();
            self.er_l.set_taps(&taps, 1.0);
            // right channel taps slightly later for width
            let taps_r: Vec<ReflectionTap> = s
                .er_taps
                .iter()
                .map(|t| ReflectionTap {
                    delay_ms: t.delay_ms * size * 1.08,
                    gain: t.gain * s.er_gain * 0.9,
                })
                .collect();
            self.er_r.set_taps(&taps_r, 1.0);

            let mut dl = s.fdn_delays_ms;
            let mut dr = s.fdn_delays_ms;
            for i in 0..4 {
                dl[i] *= size;
                dr[i] *= size * 1.07;
            }
            self.late_l.set_delays(&dl);
            self.late_r.set_delays(&dr);
            let rt = s.rt60_ms * p.decay_scale.clamp(0.5, 2.0);
            self.late_l.set_decay(rt);
            self.late_r.set_decay(rt);
            let damp = p.damping_hz.min(s.damping_hz * 1.5);
            self.late_l.set_damping_hz(damp);
            self.late_r.set_damping_hz(damp);
            self.size = size;
            self.last_spec = Some(p.room);
            self.tail_profile_gain = s.tail_gain;
        }
        self.send_gain.set(1.0);
        self.er_gain.set(p.level.clamp(0.0, 1.0) * p.er_level.clamp(0.0, 1.0));
        self.wet_gain.set(
            p.level.clamp(0.0, 1.0) * p.tail_level.clamp(0.0, 1.0) * self.tail_profile_gain,
        );
    }

    /// Process a mono send signal; produces stereo wet output.
    /// `out` receives (l, r) appended values - caller mixes.
    #[inline]
    pub fn process(&mut self, send: f32) -> (f32, f32) {
        let s = send * self.send_gain.next();
        let eg = self.er_gain.next();
        let wg = self.wet_gain.next();
        let el = self.er_l.process(s) * eg;
        let er_ = self.er_r.process(s) * eg;
        let ll = self.late_l.process(s) * wg;
        let lr = self.late_r.process(s) * wg;
        (el + ll, er_ + lr)
    }

    pub fn reset(&mut self) {
        self.er_l.reset();
        self.er_r.reset();
        self.late_l.reset();
        self.late_r.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_profiles_differ() {
        let sr = 48000.0;
        let impulse_response = |room: RoomProfile| -> f32 {
            let mut rs = Roomspace::new(sr);
            let mut p = RoomspaceParams::default();
            p.room = room;
            rs.update(&p);
            let mut e = 0.0f32;
            for i in 0..24000 {
                let x = if i == 0 { 0.5 } else { 0.0 };
                let (l, r) = rs.process(x);
                e += l * l + r * r;
            }
            e
        };
        let studio = impulse_response(RoomProfile::TreatedStudio);
        let living = impulse_response(RoomProfile::SmallLivingRoom);
        // treated studio should have notably less total reflection energy
        assert!(living > studio * 1.5, "studio {studio} living {living}");
    }

    #[test]
    fn stable_and_finite() {
        let mut rs = Roomspace::new(48000.0);
        rs.update(&RoomspaceParams::default());
        for i in 0..48000 {
            let (l, r) = rs.process(if i == 0 { 1.0 } else { 0.0 });
            assert!(l.is_finite() && r.is_finite());
        }
    }
}
