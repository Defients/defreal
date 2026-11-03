//! defreal-audio: device I/O, buffering, clock sync, offline rendering.

pub mod device;
pub mod engine;
pub mod offline;
pub mod resample;
pub mod telemetry;

pub use engine::{
    AudioEngine, EngineCommand, EngineConfig, EngineError, MonitorBranch, MonitorConfig, Pump,
    INTERNAL_RATE,
};
#[doc(hidden)]
pub use engine::dispatch;
pub use telemetry::{EngineState, SharedTelemetry, TelemetrySnapshot};
