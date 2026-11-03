//! defreal-audio: device I/O, buffering, clock sync, offline rendering.

pub mod device;
pub mod engine;
pub mod offline;
pub mod resample;
pub mod telemetry;

pub use engine::{AudioEngine, EngineCommand, EngineConfig, EngineError, INTERNAL_RATE};
pub use telemetry::{EngineState, TelemetrySnapshot};
