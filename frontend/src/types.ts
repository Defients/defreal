// Mirrors of the Rust-side serializable types.

export type MotionStateName =
  | "Neutral"
  | "Lean In"
  | "Lean Back"
  | "Turn Aside"
  | "Reposition"
  | "Return to Neutral";

export type EngineStateName = "idle" | "starting" | "running" | "stopping" | "failed";

export interface Telemetry {
  state: EngineStateName;
  error: string | null;
  in_rms_db: number;
  in_peak_db: number;
  out_rms_db: number;
  out_peak_db: number;
  limiter_gain_db: number;
  speech_level: number;
  speech_open: boolean;
  mic_comp_db: number;
  hum_comp_db: number;
  input_overflows: number;
  output_underflows: number;
  in_fill: number;
  out_fill: number;
  resample_drift_ppm: number;
  proc_avg_us: number;
  proc_max_us: number;
  est_latency_ms: number;
  motion_state: MotionStateName;
  motion_distance_m: number;
  motion_target_m: number;
  motion_progress: number;
  motion_age_s: number;
  motion_manual: boolean;
  motion_pending: string | null;
  in_rate: number;
  out_rate: number;
  in_channels: number;
  out_channels: number;
}

export interface DeviceInfo {
  name: string;
  is_default: boolean;
  min_rate: number;
  max_rate: number;
  default_channels: number;
  default_rate: number;
}

export type MicProfile =
  | "neutral"
  | "clean-desk-mic"
  | "casual-headset"
  | "webcam-mic"
  | "warm-broadcast";
export type RoomProfile = "bedroom" | "home-office" | "treated-studio" | "small-living-room";
export type AmbienceTexture = "none" | "room-tone" | "computer-fan" | "ventilation" | "electric-hum";
export type PositionMode = "auto" | "manual";

export interface ChainParams {
  input_gain_db: number;
  output_gain_db: number;
  master_amount: number;
  realism: number;
  bypass: boolean;
  micprint: {
    enabled: boolean;
    profile: MicProfile;
    hp_hz: number;
    lp_hz: number;
    presence_db: number;
    proximity_db: number;
    saturation: number;
    compression: number;
    color_amount: number;
    trim_db: number;
  };
  humanize: {
    enabled: boolean;
    compression: number;
    attack_ms: number;
    release_ms: number;
    expander: number;
    wander: number;
    saturation: number;
  };
  proximity: {
    enabled: boolean;
    mode: PositionMode;
    manual_distance_m: number;
    manual_angle_deg: number;
    direct_trim_db: number;
    room_coupling: number;
  };
  roomspace: {
    enabled: boolean;
    room: RoomProfile;
    level: number;
    size: number;
    decay_scale: number;
    er_level: number;
    tail_level: number;
    damping_hz: number;
  };
  ambience: {
    enabled: boolean;
    texture: AmbienceTexture;
    level_db: number;
    brightness: number;
    seed: number;
  };
  motion: {
    enabled: boolean;
    activity: number;
    range: number;
    speed: number;
    speech_aware: boolean;
    seed: number;
  };
}

export interface PresetMeta {
  name: string;
  factory: boolean;
  description: string;
}

export interface AppConfig {
  input_device: string | null;
  output_device: string | null;
  last_preset: string;
  block_ms: number;
  prebuffer_ms: number;
  reduced_motion: boolean;
  window: [number, number] | null;
}

export interface RenderReport {
  input_path: string;
  output_path: string;
  in_samples: number;
  out_frames: number;
  in_rms_db: number;
  out_rms_db: number;
  out_peak_db: number;
  in_rate: number;
  level_match_db: number;
}

export interface AbResult {
  a_path: string;
  b_path: string;
  b_matched_path: string;
  a_rms_db: number;
  b_rms_db: number;
  level_match_db: number;
  frames: number;
}
