//! Natural Motion Engine — the simulated human at the mic.
//!
//! A seeded, speech-aware state machine producing a slowly-varying
//! `PositionState` that drives PROXIMITY/ROOMSPACE/MICPRINT coherently.
//!
//! Design principles:
//!  - neutral most of the time, occasional coherent excursions
//!  - smooth ease-in-out transitions, never teleports
//!  - prefers to start moving near pauses when speech-aware
//!  - deterministic given (seed, scripted speech input)
//!  - manual override pauses automation cleanly

use crate::rng::Rng;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MotionState {
    Neutral,
    LeanIn,
    LeanBack,
    TurnAside,
    Reposition,
    ReturnToNeutral,
}

impl MotionState {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Neutral => "Neutral",
            Self::LeanIn => "Lean In",
            Self::LeanBack => "Lean Back",
            Self::TurnAside => "Turn Aside",
            Self::Reposition => "Reposition",
            Self::ReturnToNeutral => "Return to Neutral",
        }
    }
}

/// The position the DSP modules consume.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PositionState {
    /// speaker-to-mic distance, meters
    pub distance_m: f32,
    /// speaker horizontal offset relative to mic axis, degrees
    pub azimuth_deg: f32,
    /// speaker facing rotation (0 = facing the mic), degrees
    pub facing_deg: f32,
}

impl PositionState {
    pub const NEUTRAL: Self = Self { distance_m: 0.32, azimuth_deg: 0.0, facing_deg: 0.0 };

    fn lerp(&self, other: &Self, t: f32) -> Self {
        Self {
            distance_m: self.distance_m + (other.distance_m - self.distance_m) * t,
            azimuth_deg: self.azimuth_deg + (other.azimuth_deg - self.azimuth_deg) * t,
            facing_deg: self.facing_deg + (other.facing_deg - self.facing_deg) * t,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct MotionParams {
    /// automation on/off (manual positioning still works when off)
    pub enabled: bool,
    /// how frequently movement happens, 0..1
    pub activity: f32,
    /// excursion size scale, 0..1
    pub range: f32,
    /// transition speed multiplier, 0.5..2.0
    pub speed: f32,
    /// prefer to start moves during speech pauses
    pub speech_aware: bool,
    pub seed: u64,
}

impl Default for MotionParams {
    fn default() -> Self {
        Self {
            enabled: true,
            activity: 0.35,
            range: 0.7,
            speed: 1.0,
            speech_aware: true,
            seed: 0xDEF1EA7,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TransitionRecord {
    pub state: MotionState,
    pub at_seconds: f32,
}

const HISTORY_LEN: usize = 24;

pub struct MotionEngine {
    rng: Rng,
    state: MotionState,
    /// smoothed/interpolated output
    current: PositionState,
    from: PositionState,
    target: PositionState,
    transition_pos: f32,   // 0..1
    transition_dur: f32,   // seconds
    dwell: f32,            // seconds left in state before next decision
    cooldown: f32,         // min time after transition before re-deciding
    pending: Option<(MotionState, f32)>, // (state, max_wait_s)
    time: f32,
    manual: bool,
    manual_pos: PositionState,
    history: VecDeque<TransitionRecord>,
    last_transition_at: f32,
}

impl MotionEngine {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: Rng::new(seed),
            state: MotionState::Neutral,
            current: PositionState::NEUTRAL,
            from: PositionState::NEUTRAL,
            target: PositionState::NEUTRAL,
            transition_pos: 1.0,
            transition_dur: 1.0,
            dwell: 6.0,
            cooldown: 0.0,
            pending: None,
            time: 0.0,
            manual: false,
            manual_pos: PositionState::NEUTRAL,
            history: VecDeque::with_capacity(HISTORY_LEN),
            last_transition_at: 0.0,
        }
    }

    pub fn reseed(&mut self, seed: u64) {
        self.rng = Rng::new(seed);
    }

    /// Take manual control (automation paused).
    pub fn set_manual(&mut self, pos: PositionState) {
        self.manual = true;
        self.manual_pos = pos;
        self.begin_transition_to(pos, MotionState::Reposition, 1.2);
    }

    /// Resume automation at neutral.
    pub fn release_manual(&mut self) {
        self.manual = false;
        self.begin_transition_to(PositionState::NEUTRAL, MotionState::ReturnToNeutral, 1.6);
    }

    /// Force a state (UI "trigger" buttons / tests).
    pub fn trigger(&mut self, state: MotionState) {
        let target = self.pose_for(state);
        let dur = self.pick_duration(state);
        self.begin_transition_to(target, state, dur);
        self.pending = None;
    }

    fn pose_for(&mut self, state: MotionState) -> PositionState {
        let r = self.rng.f32();
        match state {
            MotionState::Neutral | MotionState::ReturnToNeutral => PositionState {
                distance_m: 0.30 + r * 0.06,
                azimuth_deg: (self.rng.f32() - 0.5) * 6.0,
                facing_deg: 0.0,
            },
            MotionState::LeanIn => PositionState {
                distance_m: 0.17 + r * 0.06,
                azimuth_deg: (self.rng.f32() - 0.5) * 8.0,
                facing_deg: (self.rng.f32() - 0.5) * 6.0,
            },
            MotionState::LeanBack => PositionState {
                distance_m: 0.6 + r * 0.25,
                azimuth_deg: (self.rng.f32() - 0.5) * 16.0,
                facing_deg: 6.0 + self.rng.f32() * 12.0,
            },
            MotionState::TurnAside => {
                let side = if self.rng.chance(0.5) { -1.0 } else { 1.0 };
                PositionState {
                    distance_m: 0.40 + r * 0.15,
                    azimuth_deg: side * (25.0 + self.rng.f32() * 25.0),
                    facing_deg: 20.0 + self.rng.f32() * 25.0,
                }
            }
            MotionState::Reposition => PositionState {
                distance_m: 0.30 + r * 0.2,
                azimuth_deg: (self.rng.f32() - 0.5) * 30.0,
                facing_deg: (self.rng.f32() - 0.5) * 20.0,
            },
        }
    }

    fn pick_duration(&mut self, state: MotionState) -> f32 {
        let base = match state {
            MotionState::LeanIn => 0.7,
            MotionState::Reposition => 1.3,
            MotionState::ReturnToNeutral | MotionState::Neutral => 1.6,
            MotionState::LeanBack | MotionState::TurnAside => 1.1,
        };
        base * (0.8 + self.rng.f32() * 0.5)
    }

    fn begin_transition_to(&mut self, target: PositionState, state: MotionState, dur: f32) {
        self.from = self.current;
        self.target = target;
        self.state = state;
        self.transition_pos = 0.0;
        self.transition_dur = dur.max(0.15);
        self.cooldown = dur + 1.0;
        self.dwell = self.pick_dwell(state);
        self.last_transition_at = self.time;
        if self.history.len() >= HISTORY_LEN {
            self.history.pop_front();
        }
        self.history.push_back(TransitionRecord { state, at_seconds: self.time });
    }

    fn pick_dwell(&mut self, state: MotionState) -> f32 {
        match state {
            MotionState::Neutral => 5.0 + self.rng.f32() * 9.0,
            MotionState::LeanIn => 1.5 + self.rng.f32() * 3.5,
            MotionState::LeanBack => 2.5 + self.rng.f32() * 5.0,
            MotionState::TurnAside => 1.0 + self.rng.f32() * 2.5,
            MotionState::Reposition => 1.0 + self.rng.f32() * 2.0,
            MotionState::ReturnToNeutral => 4.0 + self.rng.f32() * 8.0,
        }
    }

    /// Decide the next state. Weighted; loud sustained speech makes LeanIn
    /// more likely, but default stays restrained.
    fn decide_next(&mut self, speech_level: f32) -> MotionState {
        match self.state {
            MotionState::Neutral | MotionState::ReturnToNeutral => {
                let loud = (speech_level * 6.0).clamp(0.0, 1.0);
                let roll = self.rng.f32();
                if roll < 0.18 + loud * 0.30 {
                    MotionState::LeanIn
                } else if roll < 0.55 {
                    MotionState::LeanBack
                } else if roll < 0.78 {
                    MotionState::Reposition
                } else {
                    MotionState::TurnAside
                }
            }
            // any excursion usually returns toward neutral
            _ => {
                if self.rng.chance(0.78) {
                    MotionState::ReturnToNeutral
                } else {
                    // occasional chained move
                    match self.rng.below(3) {
                        0 => MotionState::Reposition,
                        1 => MotionState::LeanBack,
                        _ => MotionState::TurnAside,
                    }
                }
            }
        }
    }

    /// Advance the engine. Call once per processing block.
    /// `speech_open`: VAD gate state. `speech_level`: smoothed envelope (lin).
    pub fn update(&mut self, dt: f32, speech_open: bool, speech_level: f32, p: &MotionParams) -> PositionState {
        self.time += dt;

        // manual mode: glide toward the manually-held position
        if self.manual {
            self.advance_transition(dt, p.speed);
            self.current = self.from.lerp(&self.manual_pos, ease(self.transition_pos));
            return self.current;
        }

        // scale pose targets by range
        if !p.enabled {
            // automation off: drift gently back toward neutral, stay put
            self.advance_transition(dt, p.speed);
            self.current = self.from.lerp(&self.target, ease(self.transition_pos));
            return self.current;
        }

        if self.cooldown > 0.0 {
            self.cooldown -= dt;
        }
        if self.dwell > 0.0 && self.transition_pos >= 1.0 {
            self.dwell -= dt;
        }

        // pending move waits for a pause (bounded wait)
        if let Some((state, wait)) = self.pending.take() {
            if wait <= 0.0 || !speech_open {
                let mut pose = self.pose_for(state);
                self.scale_pose(&mut pose, p.range);
                let dur = self.pick_duration(state) / p.speed.max(0.2);
                self.begin_transition_to(pose, state, dur);
            } else {
                self.pending = Some((state, wait - dt));
            }
        } else if self.dwell <= 0.0 && self.cooldown <= 0.0 && self.transition_pos >= 1.0 {
            // activity controls whether a move happens at all this window
            let move_prob = 0.25 + p.activity * 0.7;
            if self.rng.chance(move_prob) {
                let next = self.decide_next(speech_level);
                if p.speech_aware && speech_open && next != MotionState::LeanIn {
                    // wait for a pause for non-emphasis moves
                    self.pending = Some((next, 1.5 + self.rng.f32()));
                } else {
                    let mut pose = self.pose_for(next);
                    self.scale_pose(&mut pose, p.range);
                    let dur = self.pick_duration(next) / p.speed.max(0.2);
                    self.begin_transition_to(pose, next, dur);
                }
            } else {
                // decided to stay; shorter re-check window
                self.dwell = 1.5 + self.rng.f32() * 4.0;
            }
        }

        self.advance_transition(dt, p.speed);
        self.current = self.from.lerp(&self.target, ease(self.transition_pos));

        // ReturnToNeutral is a *transition*, not a place to live: once the
        // glide completes, we are Neutral again (dwell then counts normally).
        if self.transition_pos >= 1.0 && self.state == MotionState::ReturnToNeutral {
            self.state = MotionState::Neutral;
            if self.dwell <= 0.0 {
                self.dwell = self.pick_dwell(MotionState::Neutral);
            }
        }
        self.current
    }

    fn scale_pose(&self, pose: &mut PositionState, range: f32) {
        let r = range.clamp(0.1, 1.0);
        // scale distance excursions around neutral
        pose.distance_m = PositionState::NEUTRAL.distance_m
            + (pose.distance_m - PositionState::NEUTRAL.distance_m) * r;
        pose.azimuth_deg *= r;
        pose.facing_deg *= r;
        pose.distance_m = pose.distance_m.clamp(0.12, 3.0);
    }

    fn advance_transition(&mut self, dt: f32, _speed: f32) {
        if self.transition_pos < 1.0 {
            self.transition_pos = (self.transition_pos + dt / self.transition_dur).min(1.0);
        }
    }

    pub fn current(&self) -> PositionState {
        self.current
    }
    pub fn state(&self) -> MotionState {
        self.state
    }
    pub fn target_distance(&self) -> f32 {
        self.target.distance_m
    }
    pub fn transition_progress(&self) -> f32 {
        self.transition_pos
    }
    pub fn seconds_since_transition(&self) -> f32 {
        self.time - self.last_transition_at
    }
    pub fn is_manual(&self) -> bool {
        self.manual
    }
    pub fn history(&self) -> &VecDeque<TransitionRecord> {
        &self.history
    }

    pub fn debug(&self) -> MotionDebug {
        MotionDebug {
            state: self.state,
            distance_m: self.current.distance_m,
            target_distance_m: self.target.distance_m,
            transition_progress: self.transition_pos,
            seconds_since_transition: self.seconds_since_transition(),
            manual: self.manual,
            pending: self.pending.map(|(s, _)| s),
        }
    }
}

#[derive(Clone, Debug)]
pub struct MotionDebug {
    pub state: MotionState,
    pub distance_m: f32,
    pub target_distance_m: f32,
    pub transition_progress: f32,
    pub seconds_since_transition: f32,
    pub manual: bool,
    pub pending: Option<MotionState>,
}

#[inline]
fn ease(t: f32) -> f32 {
    // smoothstep
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> MotionParams {
        MotionParams { enabled: true, activity: 0.8, range: 1.0, speed: 1.0, speech_aware: false, seed: 42 }
    }

    #[test]
    fn deterministic_with_seed() {
        let mut a = MotionEngine::new(7);
        let mut b = MotionEngine::new(7);
        let p = params();
        for i in 0..48000 {
            let level = if (i / 4800) % 3 == 0 { 0.1 } else { 0.0 };
            let pa = a.update(0.001, level > 0.0, level, &p);
            let pb = b.update(0.001, level > 0.0, level, &p);
            assert_eq!(pa.distance_m, pb.distance_m);
            assert_eq!(pa.azimuth_deg, pb.azimuth_deg);
        }
    }

    #[test]
    fn movement_is_smooth_no_teleports() {
        let mut m = MotionEngine::new(11);
        let p = params();
        let mut prev = m.current().distance_m;
        for i in 0..96000 {
            let speech = (i / 4800) % 4 != 0;
            let level = if speech { 0.12 } else { 0.0 };
            let pos = m.update(0.001, speech, level, &p);
            let delta = (pos.distance_m - prev).abs();
            // at 1ms steps, max plausible speed is ~1m/0.15s => 6.7mm/ms
            assert!(delta < 0.01, "jump of {delta} at i={i}");
            prev = pos.distance_m;
        }
    }

    #[test]
    fn mostly_neutral_over_long_run() {
        let mut m = MotionEngine::new(5);
        let p = MotionParams { activity: 0.3, ..params() };
        let mut neutral_time = 0u32;
        let total = 600_000; // 10 min at 1ms
        for i in 0..total {
            let speech = (i / 2400) % 5 != 0;
            let level = if speech { 0.1 } else { 0.0 };
            m.update(0.001, speech, level, &p);
            if m.state() == MotionState::Neutral {
                neutral_time += 1;
            }
        }
        let frac = neutral_time as f32 / total as f32;
        assert!(frac > 0.35, "neutral frac {frac}");
    }

    #[test]
    fn manual_override_holds_position() {
        let mut m = MotionEngine::new(1);
        let p = params();
        m.set_manual(PositionState { distance_m: 1.0, azimuth_deg: 10.0, facing_deg: 5.0 });
        for _ in 0..5000 {
            m.update(0.001, false, 0.0, &p);
        }
        assert!((m.current().distance_m - 1.0).abs() < 0.01);
        m.release_manual();
        for _ in 0..8000 {
            m.update(0.001, false, 0.0, &p);
        }
        assert!(m.current().distance_m < 0.5);
    }
}
