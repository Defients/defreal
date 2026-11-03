//! ROOMLOGIC — the unified acoustic scene.
//!
//! One shared spatial model for every sound source in DefReal: the voice
//! speaker, DefDog, and SOUNDSTAGE emitters all live in the same metric
//! coordinate space (mic at origin, +x toward the hallway wall) and are
//! captured by the same virtual microphone.
//!
//! This is a *perceptually motivated* approximation — not ray tracing, not
//! measured acoustics. Cross-room propagation resolves to three regimes:
//! direct (same room), doorway (open door), transmitted (closed door).
//! Geometry is evaluated at control rate; the audio path sees only smooth
//! {gain, lp_hz, room_send} parameter updates.

use serde::{Deserialize, Serialize};

/// Meters. Mic sits at origin; the wall dividing studio and hallway is
/// the plane x = WALL_X. The doorway opening is centered at y=0 on it.
pub const WALL_X: f32 = 1.5;
pub const DOOR_Y: f32 = 0.0;
pub const DOOR_HALF_W: f32 = 0.45;

/// Scene bounds for emitter/source placement.
pub const X_MIN: f32 = -1.2;
pub const X_MAX: f32 = 3.4;
pub const Y_MIN: f32 = -1.6;
pub const Y_MAX: f32 = 1.6;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoomId {
    Studio,
    Hallway,
}

/// Which room does a point belong to? Wall plane at x = WALL_X.
pub fn room_of(x: f32) -> RoomId {
    if x > WALL_X { RoomId::Hallway } else { RoomId::Studio }
}

pub fn clamp_pos(x: f32, y: f32) -> (f32, f32) {
    (x.clamp(X_MIN, X_MAX), y.clamp(Y_MIN, Y_MAX))
}

/// Propagation regime for a source at (x,y) heard by the mic (origin).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropKind {
    /// same room: unobstructed
    Direct,
    /// other room, door open: portal-mediated
    Doorway,
    /// other room, door closed: transmitted through the barrier
    Transmitted,
}

/// Derived acoustic parameters for one source → mic propagation.
/// Consumed by the DSP layer; smooth via SmoothedParam at the caller.
#[derive(Clone, Copy, Debug)]
pub struct Prop {
    pub kind: PropKind,
    /// extra gain applied on top of distance attenuation
    pub gain: f32,
    /// extra low-pass applied to the direct path (Hz)
    pub lp_hz: f32,
    /// multiplier on the source's room-send
    pub send_mul: f32,
    /// effective distance for attenuation (door detour adds path length)
    pub eff_distance_m: f32,
}

/// Compute propagation for a source at (x,y) with the door at `open` 0..1.
/// Mic is fixed in the studio at origin.
pub fn propagate(x: f32, y: f32, door_open: f32) -> Prop {
    let door = door_open.clamp(0.0, 1.0);
    if room_of(x) == RoomId::Studio {
        return Prop {
            kind: PropKind::Direct,
            gain: 1.0,
            lp_hz: 17000.0,
            send_mul: 1.0,
            eff_distance_m: (x * x + y * y).sqrt().max(0.08),
        };
    }
    // cross-room: blend closed-transmission -> open-doorway by door position.
    // closed: heavy LP + -13dB; open: mild LP + -1.5dB. Never absolute silence.
    let t = door;
    let gain = 0.22 + (0.85 - 0.22) * t;
    let lp_hz = 1300.0 + (8200.0 - 1300.0) * t;
    let send_mul = 0.55 + (1.15 - 0.55) * t;
    // door adds path length: source -> door -> mic
    let door_detour = (((x - WALL_X).powi(2) + (y - DOOR_Y).powi(2)).sqrt()
        + (WALL_X.powi(2) + DOOR_Y.powi(2)).sqrt())
        .max(0.08);
    let straight = (x * x + y * y).sqrt().max(0.08);
    let eff = straight * (1.0 - t) + door_detour * t;
    Prop {
        kind: if t > 0.5 { PropKind::Doorway } else { PropKind::Transmitted },
        gain,
        lp_hz,
        send_mul,
        eff_distance_m: eff,
    }
}

/// Scene configuration as held by the UI/persistence (not RT state).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct SceneConfig {
    pub enabled: bool,
    /// 0.0 = closed, 1.0 = fully open (smoothed engine-side)
    pub door_open: f32,
    /// adjacent room's acoustic character
    pub room_b: crate::roomspace::RoomProfile,
}

impl Default for SceneConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            door_open: 1.0,
            room_b: crate::roomspace::RoomProfile::SmallLivingRoom,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_membership_and_clamp() {
        assert_eq!(room_of(0.0), RoomId::Studio);
        assert_eq!(room_of(1.4), RoomId::Studio);
        assert_eq!(room_of(1.6), RoomId::Hallway);
        assert_eq!(clamp_pos(-9.0, 9.0), (X_MIN, Y_MAX));
    }

    #[test]
    fn same_room_is_direct() {
        let p = propagate(0.5, 0.5, 0.0);
        assert_eq!(p.kind, PropKind::Direct);
        assert_eq!(p.gain, 1.0);
        assert_eq!(p.lp_hz, 17000.0);
    }

    #[test]
    fn open_door_is_clearer_than_closed() {
        let open = propagate(2.5, 0.3, 1.0);
        let closed = propagate(2.5, 0.3, 0.0);
        assert_eq!(open.kind, PropKind::Doorway);
        assert_eq!(closed.kind, PropKind::Transmitted);
        assert!(open.gain > closed.gain);
        assert!(open.lp_hz > closed.lp_hz);
        // closed is muffled, not silent
        assert!(closed.gain > 0.1);
        // open-door hallway path is longer (door detour)
        assert!(open.eff_distance_m >= 2.0);
    }

    #[test]
    fn door_blend_is_monotonic() {
        let mut prev_lp = 0.0f32;
        let mut prev_g = 0.0f32;
        for i in 0..=10 {
            let p = propagate(2.5, 0.2, i as f32 / 10.0);
            assert!(p.lp_hz >= prev_lp && p.gain >= prev_g);
            prev_lp = p.lp_hz;
            prev_g = p.gain;
        }
    }
}
