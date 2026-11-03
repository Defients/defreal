//! defreal-dsp: platform-independent real-time acoustic realism processing.
//!
//! No I/O, no device APIs, no allocation on the processing path after
//! `prepare()`/`set_sample_rate()`. Everything is deterministic given a seed.

pub mod ambience;
pub mod biquad;
pub mod chain;
pub mod delay;
pub mod dynamics;
pub mod envelope;
pub mod humanize;
pub mod limiter;
pub mod meters;
pub mod micprint;
pub mod motion;
pub mod noise;
pub mod preset;
pub mod proximity;
pub mod realism;
pub mod reverb;
pub mod rng;
pub mod roomspace;
pub mod saturate;
pub mod smooth;
pub mod vad;

pub use chain::{ChainParams, DspChain, EngineMetrics, MotionDebug};
pub use motion::{MotionState, PositionState};
pub use preset::Preset;
