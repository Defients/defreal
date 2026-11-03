//! Device enumeration and description via CPAL.
//!
//! Identity model: cpal/WASAPI does not expose stable endpoint IDs through the
//! public API (checked cpal 0.15.3 — `Device::id` doesn't exist). The best
//! available identity is therefore `name` + a duplicate-occurrence index for
//! devices sharing a display name: `DeviceInfo::id` is `"name"` for a unique
//! device or `"name#2"` for the second one with that name. Resolution accepts
//! either an id or a bare name (bare name -> first match). Renaming a device
//! still breaks the stored id — that's documented and surfaced as NotFound
//! rather than silently picking a different endpoint.

use cpal::traits::{DeviceTrait, HostTrait};
use serde::Serialize;
use thiserror::Error;

#[derive(Clone, Debug, Serialize)]
pub struct DeviceInfo {
    /// display name (for the UI)
    pub name: String,
    /// disambiguated selection id: "name" or "name#n" for duplicates
    pub id: String,
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
        id: name.clone(), // disambiguated below
        is_default: default_name.map(|n| n == name).unwrap_or(false),
        name,
        min_rate,
        max_rate,
        default_channels: def_ch,
        default_rate: def_rate,
    })
}

/// Assign `name#n` ids where a name appears more than once.
fn disambiguate(infos: &mut [DeviceInfo]) {
    for i in 0..infos.len() {
        let name = infos[i].name.clone();
        let occurrence = infos[..i].iter().filter(|x| x.name == name).count();
        if occurrence > 0 {
            infos[i].id = format!("{name}#{}", occurrence + 1);
        } else {
            // check whether a LATER device also shares the name; if so the
            // first one stays "name" (resolution order is stable anyway)
            infos[i].id = name;
        }
    }
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
    disambiguate(&mut out);
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
    disambiguate(&mut out);
    Ok(out)
}

/// Parse an id: "name" -> (name, 1st match), "name#2" -> (name, 2nd match).
fn parse_sel(sel: &str) -> (&str, usize) {
    match sel.rfind('#') {
        Some(i) if sel[i + 1..].chars().all(|c| c.is_ascii_digit()) && i + 1 < sel.len() => {
            (&sel[..i], sel[i + 1..].parse().unwrap_or(1))
        }
        _ => (sel, 1),
    }
}

fn find_in<I>(mut devs: I, sel: &str) -> Result<cpal::Device, DeviceError>
where
    I: Iterator<Item = cpal::Device>,
{
    let (name, nth) = parse_sel(sel);
    let mut seen = 0usize;
    devs.find(|d| {
        if d.name().map(|s| s == name).unwrap_or(false) {
            seen += 1;
            seen == nth
        } else {
            false
        }
    })
    .ok_or_else(|| DeviceError::NotFound(sel.to_string()))
}

/// Resolve an input device by id/name (None = system default).
pub fn find_input(sel: Option<&str>) -> Result<cpal::Device, DeviceError> {
    let host = cpal::default_host();
    match sel {
        None => host.default_input_device().ok_or(DeviceError::NoDevices("input")),
        Some(s) => {
            let devs = host
                .input_devices()
                .map_err(|e| DeviceError::Query(e.to_string()))?;
            find_in(devs, s)
        }
    }
}

/// Resolve an output device by id/name (None = system default).
pub fn find_output(sel: Option<&str>) -> Result<cpal::Device, DeviceError> {
    let host = cpal::default_host();
    match sel {
        None => host.default_output_device().ok_or(DeviceError::NoDevices("output")),
        Some(s) => {
            let devs = host
                .output_devices()
                .map_err(|e| DeviceError::Query(e.to_string()))?;
            find_in(devs, s)
        }
    }
}

/// Heuristic feedback check: same device or obvious virtual-cable self-loop
/// (input and output share the cable's name family).
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

/// Monitor-branch safety check. Returns a warning when the chosen monitor
/// output is suspicious; `None` means "looks fine".
///
/// - monitor == main output: doubled audio on one device
/// - monitor == capture input: processed audio feeds back into capture
/// - monitor looks like the input's cable partner: same-cable loop
pub fn monitor_warning(input_name: &str, main_out: &str, monitor: &str) -> Option<String> {
    if monitor == main_out {
        return Some(format!(
            "Monitor and main output are the same device (\"{monitor}\"). The processed signal would play twice on it."
        ));
    }
    if monitor == input_name {
        return Some(format!(
            "Monitor output is also the capture device (\"{monitor}\"). Processed audio would feed back into DefReal."
        ));
    }
    // reuse the cable-family heuristic between monitor and INPUT
    if let Some(w) = feedback_warning(input_name, monitor) {
        return Some(w.replace("DefReal's output", "the monitor feed"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sel_handles_plain_and_suffixed_ids() {
        assert_eq!(parse_sel("Speakers"), ("Speakers", 1));
        assert_eq!(parse_sel("Speakers#2"), ("Speakers", 2));
        assert_eq!(parse_sel("CABLE Output#3"), ("CABLE Output", 3));
        // "#name" (no digits after) is NOT a suffix — treated as literal name
        assert_eq!(parse_sel("Mic #A"), ("Mic #A", 1));
        assert_eq!(parse_sel("Mic#"), ("Mic#", 1));
    }

    #[test]
    fn disambiguate_assigns_occurrence_ids() {
        let mut v = vec![
            DeviceInfo { name: "Speakers".into(), id: "Speakers".into(), is_default: true, min_rate: 48, max_rate: 48, default_channels: 2, default_rate: 48000 },
            DeviceInfo { name: "Headset".into(), id: "Headset".into(), is_default: false, min_rate: 48, max_rate: 48, default_channels: 2, default_rate: 48000 },
            DeviceInfo { name: "Speakers".into(), id: "Speakers".into(), is_default: false, min_rate: 48, max_rate: 48, default_channels: 2, default_rate: 48000 },
            DeviceInfo { name: "Speakers".into(), id: "Speakers".into(), is_default: false, min_rate: 48, max_rate: 48, default_channels: 2, default_rate: 48000 },
        ];
        disambiguate(&mut v);
        assert_eq!(v[0].id, "Speakers");
        assert_eq!(v[1].id, "Headset");
        assert_eq!(v[2].id, "Speakers#2");
        assert_eq!(v[3].id, "Speakers#3");
        // each id round-trips through parse_sel
        assert_eq!(parse_sel(&v[3].id), ("Speakers", 3));
    }

    #[test]
    fn monitor_warning_catches_loops() {
        assert!(monitor_warning("Mic", "Speakers", "Speakers").is_some());
        assert!(monitor_warning("Mic", "Speakers", "Mic").is_some());
        assert!(monitor_warning("CABLE Output", "Speakers", "CABLE Input").is_some());
        assert!(monitor_warning("Mic", "Speakers", "Headphones").is_none());
    }
}
