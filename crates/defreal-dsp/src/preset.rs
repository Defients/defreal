//! Versioned preset schema + the five factory presets.
//!
//! Presets are plain JSON on disk. `version` gates migration; unknown fields
//! are ignored (forward-compatible), missing fields get serde defaults
//! (backward-compatible). Device routing is deliberately NOT part of the
//! preset - it lives in app config.

use crate::chain::ChainParams;
use crate::motion::PositionState;
use serde::{Deserialize, Serialize};

pub const PRESET_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Preset {
    pub version: u32,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// whether this is a factory preset (built-ins can't be deleted)
    #[serde(default)]
    pub factory: bool,
    #[serde(flatten)]
    pub params: ChainParams,
}

#[derive(Debug, thiserror::Error)]
pub enum PresetError {
    #[error("unsupported preset version {0} (max {PRESET_VERSION})")]
    UnsupportedVersion(u32),
    #[error("invalid preset: {0}")]
    Invalid(String),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

impl Preset {
    pub fn new(name: &str, params: ChainParams) -> Self {
        Self {
            version: PRESET_VERSION,
            name: name.to_string(),
            description: String::new(),
            factory: false,
            params,
        }
    }

    pub fn to_json(&self) -> Result<String, PresetError> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Parse + validate + clamp into range. Malformed -> error, never panic.
    pub fn from_json(s: &str) -> Result<Self, PresetError> {
        let mut p: Preset = serde_json::from_str(s)?;
        if p.version > PRESET_VERSION {
            return Err(PresetError::UnsupportedVersion(p.version));
        }
        p.version = PRESET_VERSION;
        p.validate()?;
        Ok(p)
    }

    /// Clamp every numeric field into its valid range.
    pub fn validate(&mut self) -> Result<(), PresetError> {
        if self.name.trim().is_empty() {
            return Err(PresetError::Invalid("empty name".into()));
        }
        let c = &mut self.params;
        clamp(&mut c.input_gain_db, -30.0, 30.0);
        clamp(&mut c.output_gain_db, -30.0, 12.0);
        clamp(&mut c.master_amount, 0.0, 1.0);
        clamp(&mut c.realism, 0.0, 1.0);

        clamp(&mut c.micprint.hp_hz, 20.0, 800.0);
        clamp(&mut c.micprint.lp_hz, 2000.0, 20000.0);
        clamp(&mut c.micprint.presence_db, -12.0, 12.0);
        clamp(&mut c.micprint.proximity_db, -6.0, 9.0);
        clamp(&mut c.micprint.saturation, 0.0, 1.0);
        clamp(&mut c.micprint.compression, 0.0, 1.0);
        clamp(&mut c.micprint.color_amount, 0.0, 1.0);
        clamp(&mut c.micprint.trim_db, -18.0, 12.0);

        clamp(&mut c.humanize.compression, 0.0, 1.0);
        clamp(&mut c.humanize.attack_ms, 1.0, 200.0);
        clamp(&mut c.humanize.release_ms, 20.0, 2000.0);
        clamp(&mut c.humanize.expander, 0.0, 1.0);
        clamp(&mut c.humanize.wander, 0.0, 1.0);
        clamp(&mut c.humanize.saturation, 0.0, 1.0);

        clamp(&mut c.proximity.manual_distance_m, 0.1, 3.0);
        clamp(&mut c.proximity.manual_angle_deg, -90.0, 90.0);
        clamp(&mut c.proximity.direct_trim_db, -18.0, 12.0);
        clamp(&mut c.proximity.room_coupling, 0.0, 1.0);

        clamp(&mut c.roomspace.level, 0.0, 1.0);
        clamp(&mut c.roomspace.size, 0.5, 1.6);
        clamp(&mut c.roomspace.decay_scale, 0.5, 2.0);
        clamp(&mut c.roomspace.er_level, 0.0, 1.0);
        clamp(&mut c.roomspace.tail_level, 0.0, 1.0);
        clamp(&mut c.roomspace.damping_hz, 800.0, 16000.0);

        clamp(&mut c.ambience.level_db, -80.0, -6.0);
        clamp(&mut c.ambience.brightness, 0.0, 1.0);

        clamp(&mut c.motion.activity, 0.0, 1.0);
        clamp(&mut c.motion.range, 0.0, 1.0);
        clamp(&mut c.motion.speed, 0.5, 2.0);
        Ok(())
    }
}

fn clamp(v: &mut f32, lo: f32, hi: f32) {
    if !v.is_finite() {
        *v = lo;
    }
    *v = v.clamp(lo, hi);
}

/// The five factory presets. Names are stable identifiers.
pub fn factory_presets() -> Vec<Preset> {
    vec![
        natural_presence(),
        casual_headset(),
        lived_in_room(),
        warm_broadcast(),
        across_the_room(),
    ]
}

fn base(name: &str, desc: &str) -> Preset {
    let mut p = Preset::new(name, ChainParams::default());
    p.factory = true;
    p.description = desc.into();
    p
}

fn natural_presence() -> Preset {
    let mut p = base(
        "Natural Presence",
        "Conservative default. Subtle mic character, gentle room, faint tone.",
    );
    p.params.realism = 0.45;
    p.params.micprint.profile = crate::micprint::MicProfile::CleanDeskMic;
    p.params.micprint.color_amount = 0.5;
    p.params.micprint.compression = 0.15;
    p.params.micprint.saturation = 0.05;
    p.params.humanize.compression = 0.3;
    p.params.humanize.wander = 0.2;
    p.params.roomspace.room = crate::roomspace::RoomProfile::HomeOffice;
    p.params.roomspace.level = 0.35;
    p.params.roomspace.er_level = 0.6;
    p.params.roomspace.tail_level = 0.4;
    p.params.ambience.texture = crate::ambience::AmbienceTexture::RoomTone;
    p.params.ambience.level_db = -54.0;
    p.params.motion.enabled = true;
    p.params.motion.activity = 0.2;
    p.params.motion.range = 0.5;
    p
}

fn casual_headset() -> Preset {
    let mut p = base(
        "Casual Headset",
        "Close-mic headset voicing, mild compression, small head movements.",
    );
    p.params.realism = 0.6;
    p.params.micprint.profile = crate::micprint::MicProfile::CasualHeadset;
    p.params.micprint.color_amount = 0.8;
    p.params.micprint.proximity_db = 2.0;
    p.params.micprint.compression = 0.35;
    p.params.humanize.compression = 0.45;
    p.params.humanize.wander = 0.35;
    p.params.roomspace.room = crate::roomspace::RoomProfile::Bedroom;
    p.params.roomspace.level = 0.25;
    p.params.roomspace.tail_level = 0.3;
    p.params.ambience.texture = crate::ambience::AmbienceTexture::ComputerFan;
    p.params.ambience.level_db = -50.0;
    p.params.motion.enabled = true;
    p.params.motion.activity = 0.45;
    p.params.motion.range = 0.45;
    p
}

fn lived_in_room() -> Preset {
    let mut p = base(
        "Lived-In Room",
        "Audible space, soft ventilation, natural seated movement.",
    );
    p.params.realism = 0.7;
    p.params.micprint.profile = crate::micprint::MicProfile::CleanDeskMic;
    p.params.micprint.color_amount = 0.6;
    p.params.humanize.compression = 0.3;
    p.params.roomspace.room = crate::roomspace::RoomProfile::SmallLivingRoom;
    p.params.roomspace.level = 0.55;
    p.params.roomspace.er_level = 0.8;
    p.params.roomspace.tail_level = 0.6;
    p.params.ambience.texture = crate::ambience::AmbienceTexture::Ventilation;
    p.params.ambience.level_db = -44.0;
    p.params.motion.enabled = true;
    p.params.motion.activity = 0.55;
    p.params.motion.range = 0.8;
    p
}

fn warm_broadcast() -> Preset {
    let mut p = base(
        "Warm Broadcast",
        "Close broadcast mic, controlled compression, near-still.",
    );
    p.params.realism = 0.55;
    p.params.micprint.profile = crate::micprint::MicProfile::WarmBroadcast;
    p.params.micprint.color_amount = 0.85;
    p.params.micprint.proximity_db = 3.0;
    p.params.micprint.compression = 0.4;
    p.params.micprint.saturation = 0.15;
    p.params.humanize.compression = 0.55;
    p.params.humanize.attack_ms = 8.0;
    p.params.humanize.release_ms = 300.0;
    p.params.humanize.wander = 0.1;
    p.params.roomspace.room = crate::roomspace::RoomProfile::TreatedStudio;
    p.params.roomspace.level = 0.2;
    p.params.roomspace.tail_level = 0.25;
    p.params.ambience.texture = crate::ambience::AmbienceTexture::RoomTone;
    p.params.ambience.level_db = -58.0;
    p.params.motion.enabled = true;
    p.params.motion.activity = 0.12;
    p.params.motion.range = 0.35;
    p
}

fn across_the_room() -> Preset {
    let mut p = base(
        "Across the Room",
        "Distant source. Less direct sound, stronger room, still intelligible.",
    );
    p.params.realism = 0.8;
    p.params.micprint.profile = crate::micprint::MicProfile::Neutral;
    p.params.micprint.color_amount = 0.4;
    p.params.proximity.mode = crate::proximity::PositionMode::Manual;
    p.params.proximity.manual_distance_m = 1.4;
    p.params.proximity.room_coupling = 1.0;
    p.params.humanize.compression = 0.25;
    p.params.roomspace.room = crate::roomspace::RoomProfile::SmallLivingRoom;
    p.params.roomspace.level = 0.7;
    p.params.roomspace.er_level = 0.9;
    p.params.roomspace.tail_level = 0.75;
    p.params.ambience.texture = crate::ambience::AmbienceTexture::RoomTone;
    p.params.ambience.level_db = -46.0;
    p.params.motion.enabled = true;
    p.params.motion.activity = 0.3;
    p.params.motion.range = 0.5;
    p
}

/// Where the motion engine should start for a preset (manual mode presets).
pub fn initial_position(p: &Preset) -> PositionState {
    match p.params.proximity.mode {
        crate::proximity::PositionMode::Manual => PositionState {
            distance_m: p.params.proximity.manual_distance_m,
            azimuth_deg: p.params.proximity.manual_angle_deg,
            facing_deg: 0.0,
        },
        crate::proximity::PositionMode::Auto => PositionState::NEUTRAL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_serialization() {
        for fp in factory_presets() {
            let json = fp.to_json().unwrap();
            let back = Preset::from_json(&json).unwrap();
            assert_eq!(back.name, fp.name);
            assert!((back.params.realism - fp.params.realism).abs() < 1e-6);
        }
    }

    #[test]
    fn validates_clamps_bad_values() {
        let mut p = Preset::new("X", ChainParams::default());
        p.params.realism = 5.0;
        p.params.micprint.hp_hz = -100.0;
        p.params.ambience.level_db = 999.0;
        p.validate().unwrap();
        assert_eq!(p.params.realism, 1.0);
        assert_eq!(p.params.micprint.hp_hz, 20.0);
        assert_eq!(p.params.ambience.level_db, -6.0);
    }

    #[test]
    fn rejects_future_version() {
        let mut p = Preset::new("X", ChainParams::default());
        p.version = 99;
        let json = p.to_json().unwrap();
        assert!(matches!(
            Preset::from_json(&json),
            Err(PresetError::UnsupportedVersion(99))
        ));
    }

    #[test]
    fn rejects_malformed_json() {
        assert!(Preset::from_json("{not json").is_err());
        assert!(Preset::from_json("{\"version\":1,\"name\":\"\"}").is_err());
    }

    #[test]
    fn all_factories_validate_and_differ() {
        let presets = factory_presets();
        assert_eq!(presets.len(), 5);
        for p in &presets {
            let mut c = p.clone();
            assert!(c.validate().is_ok());
            assert!(c.factory);
        }
        let names: Vec<_> = presets.iter().map(|p| p.name.as_str()).collect();
        assert_ne!(names[0], names[1]);
    }
}
