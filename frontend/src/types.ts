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
  motion_azimuth_deg: number;
  motion_facing_deg: number;
  motion_target_azimuth_deg: number;
  motion_target_facing_deg: number;
  motion_progress: number;
  motion_age_s: number;
  motion_manual: boolean;
  motion_pending: string | null;
  in_rate: number;
  out_rate: number;
  in_channels: number;
  out_channels: number;
  monitor_state: "disabled" | "opening" | "active" | "error";
  monitor_fill: number;
  monitor_underflows: number;
  monitor_rms_db: number;
  monitor_peak_db: number;
  monitor_gain_db: number;
  monitor_muted: boolean;
  monitor_rate: number;
  monitor_channels: number;
  monitor_device: string;
  monitor_error: string | null;
  // defdog (v0.2.0)
  dog_enabled: boolean;
  dog_state: string;
  dog_distance_m: number;
  dog_azimuth_deg: number;
  dog_moving: boolean;
  dog_rms_db: number;
  dog_peak_db: number;
  dog_last_event: string | null;
}

export interface DeviceInfo {
  name: string;
  id: string;
  is_default: boolean;
  min_rate: number;
  max_rate: number;
  default_channels: number;
  default_rate: number;
}

export type MotionMode = "auto" | "touch" | "latch";

export interface SnapshotMeta {
  name: string;
  factory: boolean;
  distance_m: number;
  azimuth_deg: number;
  facing_deg: number;
  transition_s: number;
  mode_after: string;
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
  monitor_enabled: boolean;
  monitor_device: string | null;
  monitor_gain_db: number;
  monitor_muted: boolean;
  motion_mode: string;
  touch_return_s: number;
  dog: DogConfig;
}

export type DogSize = "Small" | "Medium" | "Large";
export type DogTemperament = "Calm" | "Curious" | "Playful";
export type DogActivity = "Rare" | "Occasional" | "Frequent";
export type DogRoute = "Both" | "Main" | "Monitor";

export interface DogConfig {
  enabled: boolean;
  size: DogSize;
  temperament: DogTemperament;
  activity: DogActivity;
  volume_db: number;
  allow_bark: boolean;
  route: DogRoute;
  manual_pos: [number, number] | null;
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
