//! Device enumeration and description via CPAL.

use cpal::traits::{DeviceTrait, HostTrait};
use serde::Serialize;
use thiserror::Error;

#[derive(Clone, Debug, Serialize)]
pub struct DeviceInfo {
    /// display name (used for selection)
    pub name: String,
    /// whether this is the system default for its direction
    pub is_default: bool,
    /// supported sample rates (min/max of first range)
    pub min_rate: u32,
    pub max_rate: u32,
    /// default config summary
    pub default_channels: u16,
    pub default_rate: u32,
}

#[derive(Debug, Error)]
pub enum DeviceError {
    #[error("no audio host available")]
    NoHost,
    #[error("device not found: {0}")]
    NotFound(String),
    #[error("no {0} devices available")]
    NoDevices(&'static str),
    #[error("device query failed: {0}")]
    Query(String),
}

fn describe(d: &cpal::Device, is_input: bool, default_name: Option<&str>) -> Option<DeviceInfo> {
    let name = d.name().ok()?;
    let cfg = if is_input {
        d.default_input_config().ok()
    } else {
        d.default_output_config().ok()
    };
    let (min_rate, max_rate, def_ch, def_rate) = if let Some(c) = cfg {
        let (lo, hi) = if is_input {
            d.supported_input_configs()
                .ok()
                .and_then(|mut it| {
                    it.next().map(|r| (r.min_sample_rate().0, r.max_sample_rate().0))
                })
                .unwrap_or((c.sample_rate().0, c.sample_rate().0))
        } else {
            d.supported_output_configs()
                .ok()
                .and_then(|mut it| {
                    it.next().map(|r| (r.min_sample_rate().0, r.max_sample_rate().0))
                })
                .unwrap_or((c.sample_rate().0, c.sample_rate().0))
        };
        (lo, hi, c.channels(), c.sample_rate().0)
    } else {
        (48000, 48000, 2, 48000)
    };
    Some(DeviceInfo {
        is_default: default_name.map(|n| n == name).unwrap_or(false),
        name,
        min_rate,
        max_rate,
        default_channels: def_ch,
        default_rate: def_rate,
    })
}

pub fn list_inputs() -> Result<Vec<DeviceInfo>, DeviceError> {
    let host = cpal::default_host();
    let default_name = host.default_input_device().and_then(|d| d.name().ok());
    let mut out = Vec::new();
    let devices = host.input_devices().map_err(|e| DeviceError::Query(e.to_string()))?;
    for d in devices {
        if d.supported_input_configs().map(|mut c| c.next().is_some()).unwrap_or(false) {
            if let Some(info) = describe(&d, true, default_name.as_deref()) {
                out.push(info);
            }
        }
    }
    Ok(out)
}

pub fn list_outputs() -> Result<Vec<DeviceInfo>, DeviceError> {
    let host = cpal::default_host();
    let default_name = host.default_output_device().and_then(|d| d.name().ok());
    let mut out = Vec::new();
    let devices = host
        .output_devices()
        .map_err(|e| DeviceError::Query(e.to_string()))?;
    for d in devices {
        if d.supported_output_configs().map(|mut c| c.next().is_some()).unwrap_or(false) {
            if let Some(info) = describe(&d, false, default_name.as_deref()) {
                out.push(info);
            }
        }
    }
    Ok(out)
}

/// Resolve an input device by name (None = system default).
pub fn find_input(name: Option<&str>) -> Result<cpal::Device, DeviceError> {
    let host = cpal::default_host();
    match name {
        None => host.default_input_device().ok_or(DeviceError::NoDevices("input")),
        Some(n) => {
            let mut devs = host
                .input_devices()
                .map_err(|e| DeviceError::Query(e.to_string()))?;
            devs.find(|d| d.name().map(|s| s == n).unwrap_or(false))
                .ok_or_else(|| DeviceError::NotFound(n.to_string()))
        }
    }
}

pub fn find_output(name: Option<&str>) -> Result<cpal::Device, DeviceError> {
    let host = cpal::default_host();
    match name {
        None => host.default_output_device().ok_or(DeviceError::NoDevices("output")),
        Some(n) => {
            let mut devs = host
                .output_devices()
                .map_err(|e| DeviceError::Query(e.to_string()))?;
            devs.find(|d| d.name().map(|s| s == n).unwrap_or(false))
                .ok_or_else(|| DeviceError::NotFound(n.to_string()))
        }
    }
}

/// Heuristic feedback check: same device name or obvious virtual-cable
/// self-loop (input and output share the cable's name family).
/// Returns a human-readable warning if suspicious.
pub fn feedback_warning(input_name: &str, output_name: &str) -> Option<String> {
    if input_name == output_name {
        return Some(format!(
            "Input and output are the same device (\"{input_name}\"). This will feed DefReal's output back into itself."
        ));
    }
    // common virtual cable naming pairs
    let families = [
        ("cable input", "cable output"),
        ("cable-b input", "cable-b output"),
    ];
    let il = input_name.to_lowercase();
    let ol = output_name.to_lowercase();
    for (a, b) in families {
        if (il.contains(a) && ol.contains(b)) || (il.contains(b) && ol.contains(a)) {
            return Some(format!(
                "\"{input_name}\" -> \"{output_name}\" appears to route through the same virtual cable. Confirm you intend this (DefReal output will feed back into the cable)."
            ));
        }
    }
    None
}
