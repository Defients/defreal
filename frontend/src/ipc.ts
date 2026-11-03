// Typed wrappers around the Tauri command surface.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  AbResult,
  AppConfig,
  ChainParams,
  DeviceInfo,
  PresetMeta,
  RenderReport,
  Telemetry,
} from "./types";

export const api = {
  listDevices: () =>
    invoke<{ inputs: DeviceInfo[]; outputs: DeviceInfo[] }>("list_devices"),
  engineStart: (
    input_device: string | null,
    output_device: string | null,
    block_ms?: number,
    prebuffer_ms?: number,
    acknowledged_feedback?: boolean,
  ) =>
    invoke<void>("engine_start", {
      inputDevice: input_device,
      outputDevice: output_device,
      blockMs: block_ms,
      prebufferMs: prebuffer_ms,
      acknowledgedFeedback: acknowledged_feedback,
    }),
  engineStop: () => invoke<void>("engine_stop"),
  engineRestart: (
    input_device: string | null,
    output_device: string | null,
    block_ms?: number,
    prebuffer_ms?: number,
  ) =>
    invoke<void>("engine_restart", {
      inputDevice: input_device,
      outputDevice: output_device,
      blockMs: block_ms,
      prebufferMs: prebuffer_ms,
    }),
  engineState: () => invoke<Telemetry>("engine_state"),
  acknowledgeFeedback: () => invoke<void>("acknowledge_feedback"),
  paramsGet: () => invoke<ChainParams>("params_get"),
  paramsUpdate: (patch: unknown) => invoke<ChainParams>("params_update", { patch }),
  motionTrigger: (name: string) => invoke<void>("motion_trigger", { name }),
  motionSetManual: (distance_m: number, azimuth_deg: number, facing_deg: number, durationS?: number) =>
    invoke<void>("motion_set_manual", { distanceM: distance_m, azimuthDeg: azimuth_deg, facingDeg: facing_deg, durationS: durationS ?? null }),
  motionRelease: (durationS?: number) =>
    invoke<void>("motion_release", { durationS: durationS ?? null }),
  motionFreeze: () => invoke<void>("motion_freeze"),
  monitorSet: (enabled: boolean, device: string | null, gainDb: number, muted: boolean) =>
    invoke<void>("monitor_set", { enabled, device, gainDb, muted }),
  monitorCheck: (input: string, mainOut: string, monitor: string) =>
    invoke<string | null>("monitor_check", { input, mainOut, monitor }),
  snapshotsList: () => invoke<import("./types").SnapshotMeta[]>("snapshots_list"),
  snapshotRecall: (name: string) =>
    invoke<import("./types").SnapshotMeta>("snapshot_recall", { name }),
  snapshotSave: (
    name: string,
    distanceM: number,
    azimuthDeg: number,
    facingDeg: number,
    transitionS: number,
    modeAfter: string,
  ) =>
    invoke<string>("snapshot_save", {
      name,
      distanceM,
      azimuthDeg,
      facingDeg,
      transitionS,
      modeAfter,
    }),
  snapshotDelete: (name: string) => invoke<void>("snapshot_delete", { name }),
  dogSet: (cfg: import("./types").DogConfig) => invoke<void>("dog_set", { cfg }),
  dogCommand: (name: string) => invoke<void>("dog_command", { name }),
  dogPreview: () => invoke<void>("dog_preview"),
  engineReset: () => invoke<void>("engine_reset"),
  presetsList: () => invoke<PresetMeta[]>("presets_list"),
  presetLoad: (name: string) => invoke<ChainParams>("preset_load", { name }),
  presetSave: (name: string) => invoke<string>("preset_save", { name }),
  presetDelete: (name: string) => invoke<void>("preset_delete", { name }),
  presetRename: (oldName: string, newName: string) =>
    invoke<void>("preset_rename", { old: oldName, new: newName }),
  presetDuplicate: (name: string, newName: string) =>
    invoke<string>("preset_duplicate", { name, newName }),
  presetExport: (name: string, path: string) => invoke<void>("preset_export", { name, path }),
  presetImport: (path: string) => invoke<string>("preset_import", { path }),
  presetsResetFactory: () => invoke<void>("presets_reset_factory"),
  activePreset: () => invoke<string>("active_preset"),
  renderFile: (inputPath: string, outputPath: string, presetName: string) =>
    invoke<RenderReport>("render_file", { inputPath, outputPath, presetName }),
  renderAb: (inputPath: string, outDir: string, presetName: string) =>
    invoke<AbResult>("render_ab_cmd", { inputPath, outDir, presetName }),
  feedbackCheck: (input: string, output: string) =>
    invoke<string | null>("feedback_check", { input, output }),
  configLoad: () => invoke<AppConfig>("config_load"),
  configSave: (cfg: AppConfig) => invoke<void>("config_save", { cfg }),
};

export function onTelemetry(cb: (t: Telemetry) => void) {
  return listen<Telemetry>("defreal://telemetry", (e) => cb(e.payload));
}
