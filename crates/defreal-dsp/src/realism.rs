//! REALISM macro — the big 0-100% control.
//!
//! Design contract (documented for users):
//!  - It never multiplies every parameter by one number.
//!  - User-configured module values define the *ceiling*. The macro blends
//!    each module from a subtle "floor" voicing toward the user's values
//!    along a per-module curve. Manual tweaks therefore scale predictably
//!    instead of being overwritten.
//!  - At 0% the chain's wet/dry mix is fully dry: audibly transparent.
//!    (Distinct from BYPASS, which skips the whole chain.)
//!  - Low: mic character + ambience continuity first.
//!    Mid: position variation + room identity.
//!    High: stronger color, more movement, all within safety limits.

use crate::chain::ChainParams;

/// Smoothstep curve helpers.
fn ss(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Per-module "how much of the user's value applies" at intensity i.
/// All curves are 0 at i=0? No - see contract: floors keep low settings
/// audible-but-subtle; the global dry/wet handles true transparency.
#[derive(Clone, Copy, Debug)]
pub struct MacroWeights {
    pub micprint: f32,   // eq/color strength
    pub proximity: f32,  // movement range & room coupling
    pub room: f32,       // room level scale
    pub ambience: f32,   // texture level scale
    pub humanize: f32,   // dynamics amount
    pub motion_activity: f32, // movement frequency scale
    pub amount: f32,     // overall wet/dry
}

pub fn weights(intensity: f32) -> MacroWeights {
    let i = intensity.clamp(0.0, 1.0);
    MacroWeights {
        // color is present early but grows gently
        micprint: 0.35 + 0.65 * ss(i),
        // room arrives later and rises faster
        room: ss((i * 1.25 - 0.1).clamp(0.0, 1.0)),
        // ambience is there from the start but stays quiet
        ambience: 0.3 + 0.7 * i,
        // proximity variation meaningful from ~30%
        proximity: ss((i * 1.4 - 0.25).clamp(0.0, 1.0)),
        // gentle dynamics early, fuller later
        humanize: 0.25 + 0.75 * ss(i),
        // movement frequency: restrained until high intensity
        motion_activity: i * i,
        // overall wet/dry: linear-ish but eased at both ends
        amount: ss(i),
    }
}

/// Apply the macro to a user params set, producing *effective* params.
/// `master_amount` (user-facing) multiplies the overall wet/dry.
pub fn apply(p: &ChainParams) -> ChainParams {
    let w = weights(p.realism);
    let mut e = p.clone();

    // MICPRINT: scale coloration; leave profile + hp/lp user values
    e.micprint.color_amount = p.micprint.color_amount * w.micprint;
    e.micprint.proximity_db = p.micprint.proximity_db * w.micprint;
    e.micprint.presence_db = p.micprint.presence_db * w.micprint;
    e.micprint.saturation = p.micprint.saturation * w.micprint;
    e.micprint.compression = p.micprint.compression * w.micprint;
    // keep enabled; the global amount handles audibility

    // PROXIMITY: room coupling and trim scale; mode untouched
    e.proximity.room_coupling = p.proximity.room_coupling * w.proximity;

    // ROOMSPACE: level + decay scale
    e.roomspace.level = p.roomspace.level * w.room;
    e.roomspace.er_level = p.roomspace.er_level * (0.5 + 0.5 * w.room);

    // AMBIENCE: level in dB floor-lifted toward user setting
    if p.ambience.level_db.is_finite() {
        let floor_db = -60.0;
        e.ambience.level_db = floor_db + (p.ambience.level_db - floor_db) * w.ambience;
    }

    // HUMANIZE: compression/expander/wander scale
    e.humanize.compression = p.humanize.compression * w.humanize;
    e.humanize.expander = p.humanize.expander * w.humanize;
    e.humanize.wander = p.humanize.wander * w.humanize;
    e.humanize.saturation = p.humanize.saturation * w.humanize;

    // MOTION: activity & range scale
    e.motion.activity = p.motion.activity * (0.15 + 0.85 * w.motion_activity);
    e.motion.range = p.motion.range * (0.4 + 0.6 * w.proximity);

    // master wet/dry handled at chain level
    e
}

/// Effective overall wet/dry: macro amount * user's master amount.
pub fn wet_dry(p: &ChainParams) -> f32 {
    weights(p.realism).amount * p.master_amount.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::ChainParams;

    #[test]
    fn zero_intensity_is_transparent() {
        let mut p = ChainParams::default();
        p.realism = 0.0;
        assert_eq!(wet_dry(&p), 0.0);
        let e = apply(&p);
        assert!(e.roomspace.level < 0.01);
    }

    #[test]
    fn full_intensity_returns_user_values() {
        let mut p = ChainParams::default();
        p.realism = 1.0;
        p.roomspace.level = 0.8;
        let e = apply(&p);
        assert!((e.roomspace.level - 0.8 * weights(1.0).room).abs() < 0.01);
        assert!((wet_dry(&p) - p.master_amount).abs() < 1e-6);
    }

    #[test]
    fn curves_monotonic() {
        let mut prev = -1.0f32;
        for k in 0..=20 {
            let i = k as f32 / 20.0;
            let a = weights(i).amount;
            assert!(a >= prev);
            prev = a;
        }
    }
}
