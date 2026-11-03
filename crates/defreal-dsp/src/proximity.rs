//! PROXIMITY — speaker-to-microphone relationship.
//!
//! Consumes a `PositionState` produced by the motion engine (or set manually)
//! and turns it into coordinated acoustic behaviour:
//!   - direct-path attenuation ~ 1/d
//!   - distance + off-axis high-frequency softening (air absorption, mic
//!     polar pattern)
//!   - distance-dependent loss of proximity bass (reported to MICPRINT)
//!   - relative room-send level (reported to ROOMSPACE)
//!
//! One position -> many coordinated parameters. Nothing here randomizes
//! independently.

use crate::biquad::{Biquad, BiquadKind};
use crate::motion::PositionState;
use crate::smooth::SmoothedParam;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PositionMode {
    /// Follow the motion engine.
    Auto,
    /// Hold `manual_distance_m` / `manual_angle_deg`.
    Manual,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ProximityParams {
    pub enabled: bool,
    pub mode: PositionMode,
    /// manual position (used in Manual mode; also start point for Auto)
    pub manual_distance_m: f32,
    pub manual_angle_deg: f32,
    /// additional fixed gain trim on the direct path, dB
    pub direct_trim_db: f32,
    /// how strongly distance affects the room send, 0..1
    pub room_coupling: f32,
}

impl Default for ProximityParams {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: PositionMode::Auto,
            manual_distance_m: 0.35,
            manual_angle_deg: 0.0,
            direct_trim_db: 0.0,
            room_coupling: 0.8,
        }
    }
}

/// Reference distance: where a person normally sits from a desktop mic.
pub const REF_DISTANCE_M: f32 = 0.3;

/// Convert a position into acoustic parameters. Pure function - unit tested.
#[derive(Clone, Copy, Debug)]
pub struct ProximityDerived {
    /// linear gain applied to the direct path
    pub direct_gain: f32,
    /// low-pass Hz for direct path (air absorption + off-axis)
    pub lp_hz: f32,
    /// extra (or reduced) low shelf dB for proximity bass; negative = thinner
    pub proximity_bass_db: f32,
    /// room send multiplier relative to base room level
    pub room_send: f32,
}

pub fn derive_from_position(pos: &PositionState, room_coupling: f32) -> ProximityDerived {
    let d = pos.distance_m.clamp(0.08, 3.5);
    let az = pos.azimuth_deg.to_radians().abs();
    let facing = pos.facing_deg.to_radians().abs();

    // direct amplitude: inverse-ish, softened near-field
    let direct_gain = (REF_DISTANCE_M / d).powf(0.9).clamp(0.12, 2.2);

    // off-axis angle between speaker facing direction and mic axis
    let off_axis = (az + facing) * 0.5;
    // air absorption + polar pattern: HF loss grows with distance & angle
    let distance_loss = (d / REF_DISTANCE_M - 1.0).max(0.0) * 2500.0;
    let angle_loss = (off_axis / (90f32.to_radians())) * 6000.0;
    let lp_hz = (17000.0 - distance_loss - angle_loss).clamp(3000.0, 17000.0);

    // proximity bass: strong close, gone beyond ~0.8m
    let proximity_bass_db = if d < 0.5 {
        (0.5 - d) / 0.5 * 3.0
    } else {
        -((d - 0.5).min(1.5)) * 2.0
    };

    // room send: closer = drier, farther = wetter (bounded)
    let room_send = (1.0 + room_coupling * ((d / REF_DISTANCE_M).ln() / 2.0_f32.ln()))
        .clamp(0.4, 3.0);

    ProximityDerived { direct_gain, lp_hz, proximity_bass_db, room_send }
}

pub struct Proximity {
    direct_gain: SmoothedParam,
    lp: Biquad,
    lp_hz_current: f32,
    // last derived values (for telemetry & micprint coupling)
    pub derived: ProximityDerived,
    sr: f32,
}

impl Proximity {
    pub fn new(sr: f32) -> Self {
        Self {
            direct_gain: SmoothedParam::new(1.0, 40.0, sr),
            lp: Biquad::new(BiquadKind::LowPass { freq: 17000.0, q: 0.6 }, sr),
            lp_hz_current: 17000.0,
            derived: ProximityDerived {
                direct_gain: 1.0,
                lp_hz: 17000.0,
                proximity_bass_db: 0.0,
                room_send: 1.0,
            },
            sr,
        }
    }

    /// Update from the *current* position. Called at control rate with an
    /// already-smoothed position from the motion engine, so the large-scale
    /// glide is handled there; these ramps only de-click.
    pub fn update(&mut self, pos: &PositionState, p: &ProximityParams) {
        let d = derive_from_position(pos, p.room_coupling);
        self.derived = d;
        let trim = 10f32.powf(p.direct_trim_db / 20.0);
        self.direct_gain.set(d.direct_gain * trim);
        // retune lp without resetting state
        let target = d.lp_hz;
        if (target - self.lp_hz_current).abs() > 20.0 {
            self.lp.set(
                BiquadKind::LowPass { freq: target, q: 0.6 },
                self.sr,
            );
            self.lp_hz_current = target;
        }
    }

    /// Process the direct (dry-voice) path.
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        self.lp.process(x) * self.direct_gain.next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::motion::PositionState;

    fn pos(distance_m: f32, azimuth_deg: f32, facing_deg: f32) -> PositionState {
        PositionState { distance_m, azimuth_deg, facing_deg }
    }

    #[test]
    fn farther_is_quieter() {
        let near = derive_from_position(&pos(0.2, 0.0, 0.0), 0.8);
        let far = derive_from_position(&pos(1.5, 0.0, 0.0), 0.8);
        assert!(near.direct_gain > far.direct_gain);
    }

    #[test]
    fn farther_is_darker_and_wetter() {
        let near = derive_from_position(&pos(0.25, 0.0, 0.0), 0.8);
        let far = derive_from_position(&pos(2.0, 0.0, 0.0), 0.8);
        assert!(far.lp_hz < near.lp_hz);
        assert!(far.room_send > near.room_send);
    }

    #[test]
    fn off_axis_is_darker() {
        let front = derive_from_position(&pos(0.4, 0.0, 0.0), 0.8);
        let side = derive_from_position(&pos(0.4, 60.0, 30.0), 0.8);
        assert!(side.lp_hz < front.lp_hz);
        assert!((side.direct_gain - front.direct_gain).abs() < 0.05);
    }

    #[test]
    fn close_has_more_proximity_bass() {
        let close = derive_from_position(&pos(0.15, 0.0, 0.0), 0.8);
        let far = derive_from_position(&pos(1.2, 0.0, 0.0), 0.8);
        assert!(close.proximity_bass_db > far.proximity_bass_db);
    }
}
