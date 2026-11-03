//! defreal-app: Tauri application layer.
//!
//! Owns the AudioEngine, the param store, preset persistence and config.
//! The frontend talks to us through small validated commands; telemetry is
//! pushed via the "defreal://telemetry" event at ~15 Hz.

use defreal_audio::device;
use defreal_audio::engine::{AudioEngine, EngineCommand, EngineConfig};
use defreal_audio::offline::{render_ab, render_wav, AbResult, RenderReport};
use defreal_audio::telemetry::TelemetrySnapshot;
use defreal_dsp::motion::{MotionState, PositionState};
use defreal_dsp::preset::{factory_presets, Preset};
use defreal_dsp::ChainParams;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};

// ---------------------------------------------------------------------------
// app state
// ---------------------------------------------------------------------------

pub struct AppState {
    engine: Mutex<AudioEngine>,
    /// current preset name (for the title bar)
    active_preset: Mutex<String>,
    /// whether the user dismissed the feedback warning this session
    feedback_ack: Mutex<bool>,
    data_dir: PathBuf,
}

impl AppState {
    fn presets_dir(&self) -> PathBuf {
        self.data_dir.join("presets")
    }
    fn config_path(&self) -> PathBuf {
        self.data_dir.join("config.json")
    }
}

// ---------------------------------------------------------------------------
// config (device routing persists separately from presets)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub last_preset: String,
    pub block_ms: f32,
    pub prebuffer_ms: f32,
    pub reduced_motion: bool,
    pub window: Option<(u32, u32)>,
}

fn default_config() -> AppConfig {
    AppConfig {
        last_preset: "Natural Presence".into(),
        block_ms: 5.0,
        prebuffer_ms: 12.0,
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// commands
// ---------------------------------------------------------------------------

#[tauri::command]
fn list_devices() -> Result<serde_json::Value, String> {
    let inputs = device::list_inputs().map_err(|e| e.to_string())?;
    let outputs = device::list_outputs().map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "inputs": inputs, "outputs": outputs }))
}

#[tauri::command]
fn engine_start(
    state: State<'_, AppState>,
    input_device: Option<String>,
    output_device: Option<String>,
    block_ms: Option<f32>,
    prebuffer_ms: Option<f32>,
    acknowledged_feedback: Option<bool>,
) -> Result<(), String> {
    // feedback safety: warn on suspicious routing unless acknowledged
    if let (Some(i), Some(o)) = (&input_device, &output_device) {
        if let Some(warn) = device::feedback_warning(i, o) {
            let acked = *state.feedback_ack.lock().unwrap();
            if !acked && acknowledged_feedback != Some(true) {
                return Err(format!("FEEDBACK_WARNING::{warn}"));
            }
        }
    }
    let mut eng = state.engine.lock().unwrap();
    eng.start(EngineConfig {
        input_device,
        output_device,
        block_ms: block_ms.unwrap_or(5.0),
        prebuffer_ms: prebuffer_ms.unwrap_or(12.0),
    })
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn engine_stop(state: State<'_, AppState>) -> Result<(), String> {
    state.engine.lock().unwrap().stop();
    Ok(())
}

#[tauri::command]
fn engine_state(state: State<'_, AppState>) -> TelemetrySnapshot {
    let eng = state.engine.lock().unwrap();
    let err = eng.last_error();
    eng.telemetry().snapshot(err)
}

#[tauri::command]
fn acknowledge_feedback(state: State<'_, AppState>) {
    *state.feedback_ack.lock().unwrap() = true;
}

#[tauri::command]
fn params_get(state: State<'_, AppState>) -> ChainParams {
    let eng = state.engine.lock().unwrap();
    eng.params_handle().load().as_ref().clone()
}

/// Patch update: merge a JSON object into the current params, validate,
/// push to the engine. Returns the merged params.
#[tauri::command]
fn params_update(
    state: State<'_, AppState>,
    patch: serde_json::Value,
) -> Result<ChainParams, String> {
    let eng = state.engine.lock().unwrap();
    let cur = eng.params_handle().load().as_ref().clone();
    let mut merged = serde_json::to_value(&cur).map_err(|e| e.to_string())?;
    json_merge(&mut merged, &patch);
    let mut params: ChainParams =
        serde_json::from_value(merged).map_err(|e| format!("invalid params: {e}"))?;
    let mut wrapper = Preset::new("_live", params.clone());
    wrapper.params = params.clone();
    wrapper.validate().map_err(|e| e.to_string())?;
    params = wrapper.params;
    eng.set_params(params.clone());
    Ok(params)
}

fn json_merge(dst: &mut serde_json::Value, patch: &serde_json::Value) {
    match (dst, patch) {
        (serde_json::Value::Object(d), serde_json::Value::Object(p)) => {
            for (k, v) in p {
                json_merge(d.entry(k.clone()).or_insert(serde_json::Value::Null), v);
            }
        }
        (d, p) => *d = p.clone(),
    }
}

// --- motion -----------------------------------------------------------------

#[tauri::command]
fn motion_trigger(state: State<'_, AppState>, name: String) -> Result<(), String> {
    let s = parse_motion_state(&name).ok_or_else(|| format!("unknown state {name}"))?;
    state.engine.lock().unwrap().try_send(EngineCommand::TriggerMotion(s));
    Ok(())
}

#[tauri::command]
fn motion_set_manual(
    state: State<'_, AppState>,
    distance_m: f32,
    azimuth_deg: f32,
    facing_deg: f32,
) -> Result<(), String> {
    state.engine.lock().unwrap().try_send(EngineCommand::SetManualPosition(
        PositionState {
            distance_m: distance_m.clamp(0.1, 3.0),
            azimuth_deg: azimuth_deg.clamp(-90.0, 90.0),
            facing_deg: facing_deg.clamp(-90.0, 90.0),
        },
    ));
    Ok(())
}

#[tauri::command]
fn motion_release(state: State<'_, AppState>) -> Result<(), String> {
    state.engine.lock().unwrap().try_send(EngineCommand::ReleaseManual);
    Ok(())
}

fn parse_motion_state(s: &str) -> Option<MotionState> {
    match s.to_ascii_lowercase().replace([' ', '_'], "-").as_str() {
        "neutral" => Some(MotionState::Neutral),
        "lean-in" => Some(MotionState::LeanIn),
        "lean-back" => Some(MotionState::LeanBack),
        "turn-aside" => Some(MotionState::TurnAside),
        "reposition" => Some(MotionState::Reposition),
        "return-to-neutral" => Some(MotionState::ReturnToNeutral),
        _ => None,
    }
}

#[tauri::command]
fn engine_reset(state: State<'_, AppState>) -> Result<(), String> {
    state.engine.lock().unwrap().try_send(EngineCommand::ResetChain);
    Ok(())
}

// --- presets ----------------------------------------------------------------

#[derive(Serialize)]
struct PresetMeta {
    name: String,
    factory: bool,
    description: String,
}

#[tauri::command]
fn presets_list(state: State<'_, AppState>) -> Result<Vec<PresetMeta>, String> {
    let mut out = Vec::new();
    for p in factory_presets() {
        out.push(PresetMeta {
            name: p.name.clone(),
            factory: true,
            description: p.description.clone(),
        });
    }
    let dir = state.presets_dir();
    if let Ok(rd) = fs::read_dir(&dir) {
        for e in rd.flatten() {
            if e.path().extension().map(|x| x == "json").unwrap_or(false) {
                if let Ok(text) = fs::read_to_string(e.path()) {
                    if let Ok(p) = Preset::from_json(&text) {
                        out.push(PresetMeta {
                            name: p.name.clone(),
                            factory: false,
                            description: p.description.clone(),
                        });
                    }
                }
            }
        }
    }
    Ok(out)
}

fn preset_path(dir: &std::path::Path, name: &str) -> PathBuf {
    let safe: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' { c } else { '_' })
        .collect();
    dir.join(format!("{}.json", safe.trim()))
}

fn load_preset_by_name(state: &AppState, name: &str) -> Result<Preset, String> {
    if let Some(p) = factory_presets().into_iter().find(|p| p.name == name) {
        return Ok(p);
    }
    let path = preset_path(&state.presets_dir(), name);
    let text = fs::read_to_string(&path).map_err(|_| format!("preset not found: {name}"))?;
    Preset::from_json(&text).map_err(|e| e.to_string())
}

#[tauri::command]
fn preset_load(state: State<'_, AppState>, name: String) -> Result<ChainParams, String> {
    let p = load_preset_by_name(&state, &name)?;
    *state.active_preset.lock().unwrap() = p.name.clone();
    let eng = state.engine.lock().unwrap();
    eng.set_params(p.params.clone());
    if p.params.proximity.mode == defreal_dsp::proximity::PositionMode::Manual {
        eng.try_send(EngineCommand::SetManualPosition(PositionState {
            distance_m: p.params.proximity.manual_distance_m,
            azimuth_deg: p.params.proximity.manual_angle_deg,
            facing_deg: 0.0,
        }));
    } else {
        eng.try_send(EngineCommand::ReleaseManual);
    }
    Ok(p.params)
}

#[tauri::command]
fn preset_save(state: State<'_, AppState>, name: String) -> Result<String, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("empty preset name".into());
    }
    if factory_presets().iter().any(|p| p.name == name) {
        return Err("cannot overwrite a factory preset; use Save As".into());
    }
    let eng = state.engine.lock().unwrap();
    let params = eng.params_handle().load().as_ref().clone();
    drop(eng);
    let mut p = Preset::new(&name, params);
    p.validate().map_err(|e| e.to_string())?;
    let dir = state.presets_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = preset_path(&dir, &name);
    fs::write(&path, p.to_json().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    *state.active_preset.lock().unwrap() = name;
    Ok(path.display().to_string())
}

#[tauri::command]
fn preset_delete(state: State<'_, AppState>, name: String) -> Result<(), String> {
    if factory_presets().iter().any(|p| p.name == name) {
        return Err("factory presets cannot be deleted".into());
    }
    let path = preset_path(&state.presets_dir(), &name);
    if path.exists() {
        fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn preset_rename(state: State<'_, AppState>, old: String, new: String) -> Result<(), String> {
    if factory_presets().iter().any(|p| p.name == old) {
        return Err("factory presets cannot be renamed".into());
    }
    let dir = state.presets_dir();
    let from = preset_path(&dir, &old);
    let to = preset_path(&dir, &new);
    if !from.exists() {
        return Err(format!("preset not found: {old}"));
    }
    let mut p = load_preset_by_name(&state, &old)?;
    p.name = new.clone();
    fs::write(&to, p.to_json().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    fs::remove_file(&from).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn preset_duplicate(state: State<'_, AppState>, name: String, new_name: String) -> Result<String, String> {
    let mut p = load_preset_by_name(&state, &name)?;
    p.name = new_name.clone();
    p.factory = false;
    let dir = state.presets_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = preset_path(&dir, &new_name);
    fs::write(&path, p.to_json().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

#[tauri::command]
fn preset_export(state: State<'_, AppState>, name: String, path: String) -> Result<(), String> {
    let p = load_preset_by_name(&state, &name)?;
    fs::write(&path, p.to_json().map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

#[tauri::command]
fn preset_import(state: State<'_, AppState>, path: String) -> Result<String, String> {
    let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut p = Preset::from_json(&text).map_err(|e| e.to_string())?;
    p.factory = false;
    let dir = state.presets_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // avoid clobbering an existing user preset
    let mut name = p.name.clone();
    let mut i = 2;
    while preset_path(&dir, &name).exists() {
        name = format!("{} ({})", p.name, i);
        i += 1;
    }
    p.name = name.clone();
    let dest = preset_path(&dir, &name);
    fs::write(&dest, p.to_json().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(name)
}

#[tauri::command]
fn presets_reset_factory(state: State<'_, AppState>) -> Result<(), String> {
    // user presets live in their own files; "reset" just clears them
    let dir = state.presets_dir();
    if let Ok(rd) = fs::read_dir(&dir) {
        for e in rd.flatten() {
            let _ = fs::remove_file(e.path());
        }
    }
    preset_load(state, "Natural Presence".into()).map(|_| ())
}

#[tauri::command]
fn active_preset(state: State<'_, AppState>) -> String {
    state.active_preset.lock().unwrap().clone()
}

// --- offline render ---------------------------------------------------------

#[tauri::command]
fn render_file(
    state: State<'_, AppState>,
    input_path: String,
    output_path: String,
    preset_name: String,
) -> Result<RenderReport, String> {
    let p = load_preset_by_name(&state, &preset_name)?;
    render_wav(
        std::path::Path::new(&input_path),
        std::path::Path::new(&output_path),
        &p,
        0xDEF1EA7,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn render_ab_cmd(
    state: State<'_, AppState>,
    input_path: String,
    out_dir: String,
    preset_name: String,
) -> Result<AbResult, String> {
    let p = load_preset_by_name(&state, &preset_name)?;
    render_ab(
        std::path::Path::new(&input_path),
        std::path::Path::new(&out_dir),
        &p,
        0xDEF1EA7,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn feedback_check(input: String, output: String) -> Option<String> {
    device::feedback_warning(&input, &output)
}

// --- config ------------------------------------------------------------------

#[tauri::command]
fn config_load(state: State<'_, AppState>) -> AppConfig {
    let path = state.config_path();
    fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(default_config)
}

#[tauri::command]
fn config_save(state: State<'_, AppState>, cfg: AppConfig) -> Result<(), String> {
    fs::create_dir_all(&state.data_dir).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    fs::write(state.config_path(), text).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// telemetry emitter + app setup
// ---------------------------------------------------------------------------

fn spawn_telemetry(app: AppHandle) {
    std::thread::spawn(move || loop {
        let state = app.state::<AppState>();
        let snapshot = {
            let eng = state.engine.lock().unwrap();
            let err = eng.last_error();
            eng.telemetry().snapshot(err)
        };
        let _ = app.emit("defreal://telemetry", &snapshot);
        std::thread::sleep(std::time::Duration::from_millis(66));
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let data_dir = dirs_data();
    let _ = fs::create_dir_all(data_dir.join("presets"));

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            engine: Mutex::new(AudioEngine::new()),
            active_preset: Mutex::new("Natural Presence".into()),
            feedback_ack: Mutex::new(false),
            data_dir,
        })
        .invoke_handler(tauri::generate_handler![
            list_devices,
            engine_start,
            engine_stop,
            engine_state,
            acknowledge_feedback,
            params_get,
            params_update,
            motion_trigger,
            motion_set_manual,
            motion_release,
            engine_reset,
            presets_list,
            preset_load,
            preset_save,
            preset_delete,
            preset_rename,
            preset_duplicate,
            preset_export,
            preset_import,
            presets_reset_factory,
            active_preset,
            render_file,
            render_ab_cmd,
            feedback_check,
            config_load,
            config_save,
        ])
        .setup(|app| {
            spawn_telemetry(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running DefReal");
}

/// %APPDATA%/DefReal (roaming app data; no audio is ever stored here).
fn dirs_data() -> PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        PathBuf::from(appdata).join("DefReal")
    } else {
        std::env::temp_dir().join("defreal-data")
    }
}
