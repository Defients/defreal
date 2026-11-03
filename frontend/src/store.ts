import { create } from "zustand";
import { api, onTelemetry } from "./ipc";
import type {
  AppConfig,
  ChainParams,
  DeviceInfo,
  DogConfig,
  MotionMode,
  PresetMeta,
  SnapshotMeta,
  Telemetry,
} from "./types";

interface DistanceSample {
  t: number;
  d: number;
}

interface DefRealStore {
  // engine
  telemetry: Telemetry | null;
  devices: { inputs: DeviceInfo[]; outputs: DeviceInfo[] };
  devicesError: string | null;
  running: boolean;
  engineError: string | null;
  feedbackWarning: string | null;

  // params + presets
  params: ChainParams | null;
  presets: PresetMeta[];
  activePreset: string;
  dirty: boolean;

  // routing
  inputDevice: string | null;
  outputDevice: string | null;
  blockMs: number;
  prebufferMs: number;
  reducedMotion: boolean;

  // direct monitor
  monitorEnabled: boolean;
  monitorDevice: string | null;
  monitorGainDb: number;
  monitorMuted: boolean;
  monitorWarning: string | null;

  // live presence stage
  motionMode: MotionMode;
  touchReturnS: number;
  snapshots: SnapshotMeta[];
  stageDragging: boolean;
  stagePos: { d: number; az: number } | null; // live drag position (UI-side)

  // defdog (v0.2.0)
  dog: DogConfig;
  dogDragging: boolean;
  dogPos: { d: number; az: number } | null; // live dog drag position (UI-side)

  // motion history (last ~30s, ~15hz)
  distanceHistory: DistanceSample[];

  // ui
  activeTab: "perform" | "routing" | "presets" | "lab" | "diagnostics";
  toast: { text: string; kind: "info" | "error" } | null;

  // actions
  init: () => Promise<void>;
  refreshDevices: () => Promise<void>;
  startEngine: (ackFeedback?: boolean) => Promise<void>;
  stopEngine: () => Promise<void>;
  updateParams: (patch: unknown) => Promise<void>;
  setParam: (path: string, value: number | boolean | string) => Promise<void>;
  loadPreset: (name: string) => Promise<void>;
  savePresetAs: (name: string) => Promise<void>;
  deletePreset: (name: string) => Promise<void>;
  duplicatePreset: (name: string, newName: string) => Promise<void>;
  renamePreset: (old: string, next: string) => Promise<void>;
  importPreset: (path: string) => Promise<void>;
  exportPreset: (name: string, path: string) => Promise<void>;
  resetFactory: () => Promise<void>;
  setInput: (n: string | null) => void;
  setOutput: (n: string | null) => void;
  setToast: (t: { text: string; kind: "info" | "error" } | null) => void;
  setTab: (t: DefRealStore["activeTab"]) => void;
  triggerMotion: (name: string) => Promise<void>;
  setManual: (d: number, az: number, face: number, dur?: number) => Promise<void>;
  releaseManual: (dur?: number) => Promise<void>;
  freezeMotion: () => Promise<void>;
  setMonitor: (patch: Partial<{ enabled: boolean; device: string | null; gainDb: number; muted: boolean }>) => Promise<void>;
  checkMonitorSafety: () => Promise<void>;
  setMotionMode: (m: MotionMode) => void;
  stageDragStart: () => void;
  stageDragMove: (d: number, az: number) => void;
  stageDragEnd: () => Promise<void>;
  setDog: (patch: Partial<DogConfig>) => Promise<void>;
  dogCmd: (name: "come" | "settle" | "stay") => Promise<void>;
  dogDragStart: () => void;
  dogDragMove: (d: number, az: number) => void;
  dogDragEnd: () => Promise<void>;
  acousticAction: (name: string) => Promise<void>;
  recallSnapshot: (name: string) => Promise<void>;
  saveSnapshot: (name: string, transitionS: number, modeAfter: string) => Promise<void>;
  deleteSnapshot: (name: string) => Promise<void>;
  restartEngine: () => Promise<void>;
  persistConfig: () => Promise<void>;
}

let unlistenTelemetry: (() => void) | null = null;

export const useStore = create<DefRealStore>((set, get) => ({
  telemetry: null,
  devices: { inputs: [], outputs: [] },
  devicesError: null,
  running: false,
  engineError: null,
  feedbackWarning: null,
  params: null,
  presets: [],
  activePreset: "Natural Presence",
  dirty: false,
  inputDevice: null,
  outputDevice: null,
  blockMs: 5,
  prebufferMs: 12,
  reducedMotion: false,
  monitorEnabled: false,
  monitorDevice: null,
  monitorGainDb: -18,
  monitorMuted: false,
  monitorWarning: null,
  motionMode: "auto",
  touchReturnS: 1.6,
  snapshots: [],
  stageDragging: false,
  stagePos: null,
  dog: {
    enabled: false,
    size: "Medium",
    temperament: "Calm",
    activity: "Rare",
    volume_db: -24,
    allow_bark: false,
    route: "Both",
    manual_pos: null,
  },
  dogDragging: false,
  dogPos: null,
  distanceHistory: [],
  activeTab: "perform",
  toast: null,

  init: async () => {
    const { refreshDevices, loadPreset, persistConfig } = get();
    // telemetry subscription
    if (!unlistenTelemetry) {
      unlistenTelemetry = await onTelemetry((t) => {
        const hist = get().distanceHistory;
        const last = hist[hist.length - 1];
        const now = performance.now();
        const next =
          last && now - last.t < 50
            ? hist
            : [...hist.slice(-449), { t: now, d: t.motion_distance_m }];
        set({
          telemetry: t,
          running: t.state === "running",
          distanceHistory: next,
          engineError: t.error,
        });
      });
    }
    // params + presets + config
    try {
      const [params, presets, cfg, active, snapshots] = await Promise.all([
        api.paramsGet(),
        api.presetsList(),
        api.configLoad(),
        api.activePreset(),
        api.snapshotsList().catch(() => [] as SnapshotMeta[]),
      ]);
      set({ snapshots });
      set({
        params,
        presets,
        activePreset: active,
        inputDevice: cfg.input_device,
        outputDevice: cfg.output_device,
        blockMs: cfg.block_ms || 5,
        prebufferMs: cfg.prebuffer_ms || 12,
        reducedMotion: cfg.reduced_motion || false,
        monitorEnabled: cfg.monitor_enabled || false,
        monitorDevice: cfg.monitor_device ?? null,
        monitorGainDb: cfg.monitor_gain_db ?? -18,
        monitorMuted: cfg.monitor_muted || false,
        motionMode: (cfg.motion_mode as MotionMode) || "auto",
        touchReturnS: cfg.touch_return_s || 1.6,
        dog: cfg.dog ?? {
          enabled: false, size: "Medium", temperament: "Calm",
          activity: "Rare", volume_db: -24, allow_bark: false,
          route: "Both", manual_pos: null,
        },
      });
      if (cfg.last_preset) {
        await loadPreset(cfg.last_preset);
      }
    } catch (e) {
      set({ toast: { text: `init: ${e}`, kind: "error" } });
    }
    await refreshDevices();
    const _ = persistConfig; // referenced by callers
  },

  refreshDevices: async () => {
    try {
      const devices = await api.listDevices();
      set({ devices, devicesError: null });
    } catch (e) {
      set({ devicesError: String(e), devices: { inputs: [], outputs: [] } });
    }
  },

  startEngine: async (ackFeedback) => {
    const { inputDevice, outputDevice, blockMs, prebufferMs } = get();
    set({ engineError: null, feedbackWarning: null });
    try {
      await api.engineStart(inputDevice, outputDevice, blockMs, prebufferMs, ackFeedback);
      set({ running: true });
      await get().persistConfig();
    } catch (e) {
      const msg = String(e);
      if (msg.startsWith("FEEDBACK_WARNING::")) {
        set({ feedbackWarning: msg.slice("FEEDBACK_WARNING::".length) });
      } else {
        set({ engineError: msg });
      }
    }
  },

  stopEngine: async () => {
    await api.engineStop().catch(() => {});
    set({ running: false });
  },

  updateParams: async (patch) => {
    try {
      const params = await api.paramsUpdate(patch);
      set({ params, dirty: true });
    } catch (e) {
      set({ toast: { text: String(e), kind: "error" } });
    }
  },

  setParam: async (path, value) => {
    // nested path like "micprint.presence_db" -> {micprint:{presence_db:v}}
    const keys = path.split(".");
    let patch: Record<string, unknown> = {};
    let cur = patch;
    for (let i = 0; i < keys.length - 1; i++) {
      cur[keys[i]] = {};
      cur = cur[keys[i]] as Record<string, unknown>;
    }
    cur[keys[keys.length - 1]] = value;
    await get().updateParams(patch);
  },

  loadPreset: async (name) => {
    try {
      const params = await api.presetLoad(name);
      set({ params, activePreset: name, dirty: false });
    } catch (e) {
      set({ toast: { text: String(e), kind: "error" } });
    }
  },

  savePresetAs: async (name) => {
    try {
      await api.presetSave(name);
      const presets = await api.presetsList();
      set({ presets, activePreset: name, dirty: false });
      set({ toast: { text: `Saved "${name}"`, kind: "info" } });
    } catch (e) {
      set({ toast: { text: String(e), kind: "error" } });
    }
  },

  deletePreset: async (name) => {
    try {
      await api.presetDelete(name);
      set({ presets: await api.presetsList() });
    } catch (e) {
      set({ toast: { text: String(e), kind: "error" } });
    }
  },

  duplicatePreset: async (name, newName) => {
    try {
      await api.presetDuplicate(name, newName);
      set({ presets: await api.presetsList() });
    } catch (e) {
      set({ toast: { text: String(e), kind: "error" } });
    }
  },

  renamePreset: async (old, next) => {
    try {
      await api.presetRename(old, next);
      const presets = await api.presetsList();
      set({ presets });
      if (get().activePreset === old) set({ activePreset: next });
    } catch (e) {
      set({ toast: { text: String(e), kind: "error" } });
    }
  },

  importPreset: async (path) => {
    try {
      const name = await api.presetImport(path);
      set({ presets: await api.presetsList() });
      set({ toast: { text: `Imported "${name}"`, kind: "info" } });
    } catch (e) {
      set({ toast: { text: String(e), kind: "error" } });
    }
  },

  exportPreset: async (name, path) => {
    try {
      await api.presetExport(name, path);
      set({ toast: { text: `Exported to ${path}`, kind: "info" } });
    } catch (e) {
      set({ toast: { text: String(e), kind: "error" } });
    }
  },

  resetFactory: async () => {
    try {
      await api.presetsResetFactory();
      const [params, presets] = await Promise.all([api.paramsGet(), api.presetsList()]);
      set({ params, presets, activePreset: "Natural Presence", dirty: false });
    } catch (e) {
      set({ toast: { text: String(e), kind: "error" } });
    }
  },

  setInput: (n) => set({ inputDevice: n }),
  setOutput: (n) => set({ outputDevice: n }),
  setToast: (t) => set({ toast: t }),
  setTab: (t) => set({ activeTab: t }),

  triggerMotion: async (name) => {
    await api.motionTrigger(name).catch((e) => set({ toast: { text: String(e), kind: "error" } }));
  },

  setManual: async (d, az, face, dur) => {
    await api.motionSetManual(d, az, face, dur).catch(() => {});
  },

  releaseManual: async (dur) => {
    await api.motionRelease(dur).catch(() => {});
  },

  freezeMotion: async () => {
    await api.motionFreeze().catch(() => {});
  },

  setMonitor: async (patch) => {
    const s = get();
    const enabled = patch.enabled ?? s.monitorEnabled;
    const device = patch.device !== undefined ? patch.device : s.monitorDevice;
    const gainDb = patch.gainDb ?? s.monitorGainDb;
    const muted = patch.muted ?? s.monitorMuted;
    set({ monitorEnabled: enabled, monitorDevice: device, monitorGainDb: gainDb, monitorMuted: muted });
    if (enabled && device) {
      await get().checkMonitorSafety();
    } else {
      set({ monitorWarning: null });
    }
    await api
      .monitorSet(enabled, device, gainDb, muted)
      .catch((e) => set({ toast: { text: String(e), kind: "error" } }));
    await get().persistConfig();
  },

  checkMonitorSafety: async () => {
    const s = get();
    if (!s.monitorEnabled || !s.monitorDevice || !s.inputDevice || !s.outputDevice) {
      set({ monitorWarning: null });
      return;
    }
    const w = await api
      .monitorCheck(s.inputDevice, s.outputDevice, s.monitorDevice)
      .catch(() => null);
    set({ monitorWarning: w });
  },

  setMotionMode: (m) => {
    set({ motionMode: m });
    get().persistConfig();
  },

  stageDragStart: () => {
    set({ stageDragging: true });
  },

  stageDragMove: (d, az) => {
    set({ stagePos: { d, az } });
  },

  stageDragEnd: async () => {
    const s = get();
    set({ stageDragging: false });
    // TOUCH returns to automation after release; AUTO too (drag was a temporary
    // override); LATCH keeps the position.
    if (s.motionMode !== "latch") {
      await api.motionRelease(s.touchReturnS).catch(() => {});
    }
  },

  setDog: async (patch) => {
    const dog = { ...get().dog, ...patch };
    set({ dog });
    // manual_pos handled by dogDrag* actions; keep engine in sync for the rest
    await api.dogSet(dog).catch((e) => set({ toast: { text: String(e), kind: "error" } }));
    await get().persistConfig();
  },

  dogCmd: async (name) => {
    await api.dogCommand(name).catch((e) => set({ toast: { text: String(e), kind: "error" } }));
  },

  dogDragStart: () => {
    set({ dogDragging: true });
  },

  dogDragMove: (d, az) => {
    set({ dogPos: { d, az } });
    const dog = { ...get().dog, manual_pos: [d, az] as [number, number] };
    set({ dog });
    api.dogSet(dog).catch(() => {});
  },

  dogDragEnd: async () => {
    set({ dogDragging: false });
    await get().persistConfig();
  },

  acousticAction: async (name) => {
    const s = get();
    // deterministic poses on the shared spatial state
    const acts: Record<string, { d: number; az: number; f: number; dur: number; hold: number }> = {
      "lean-in": { d: 0.18, az: 0, f: 0, dur: 0.7, hold: 1.2 },
      "lean-back": { d: 0.85, az: 4, f: 10, dur: 1.0, hold: 1.0 },
      "turn-away": { d: 0.45, az: 35, f: 32, dur: 0.9, hold: 1.0 },
      "across-room": { d: 1.7, az: 10, f: 15, dur: 1.6, hold: 1.4 },
    };
    if (name === "return") {
      await api.motionRelease(s.touchReturnS).catch(() => {});
      return;
    }
    if (name === "freeze") {
      await api.motionFreeze().catch(() => {});
      return;
    }
    const a = acts[name];
    if (!a) return;
    // touch semantics: move, hold briefly, resume automation
    await api
      .motionSetManual(a.d, a.az, a.f, a.dur)
      .catch(() => {});
    setTimeout(() => {
      if (get().motionMode !== "latch") {
        api.motionRelease(1.2).catch(() => {});
      }
    }, (a.dur + a.hold) * 1000);
  },

  recallSnapshot: async (name) => {
    try {
      await api.snapshotRecall(name);
    } catch (e) {
      set({ toast: { text: String(e), kind: "error" } });
    }
  },

  saveSnapshot: async (name, transitionS, modeAfter) => {
    const s = get();
    const t = s.telemetry;
    const d = s.stagePos?.d ?? t?.motion_distance_m ?? 0.32;
    const az = s.stagePos?.az ?? t?.motion_azimuth_deg ?? 0;
    const f = t?.motion_facing_deg ?? 0;
    try {
      await api.snapshotSave(name, d, az, f, transitionS, modeAfter);
      const snapshots = await api.snapshotsList();
      set({ snapshots });
      set({ toast: { text: `Snapshot "${name}" saved`, kind: "info" } });
    } catch (e) {
      set({ toast: { text: String(e), kind: "error" } });
    }
  },

  deleteSnapshot: async (name) => {
    try {
      await api.snapshotDelete(name);
      set({ snapshots: await api.snapshotsList() });
    } catch (e) {
      set({ toast: { text: String(e), kind: "error" } });
    }
  },

  restartEngine: async () => {
    const s = get();
    if (!s.inputDevice || !s.outputDevice) return;
    try {
      await api.engineRestart(s.inputDevice, s.outputDevice, s.blockMs, s.prebufferMs);
      set({ running: true, engineError: null });
      // re-apply monitor after restart (branch state lives in engine thread)
      if (s.monitorEnabled && s.monitorDevice) {
        await api.monitorSet(true, s.monitorDevice, s.monitorGainDb, s.monitorMuted);
      }
    } catch (e) {
      set({ engineError: String(e), running: false });
    }
  },

  persistConfig: async () => {
    const s = get();
    const cfg: AppConfig = {
      input_device: s.inputDevice,
      output_device: s.outputDevice,
      last_preset: s.activePreset,
      block_ms: s.blockMs,
      prebuffer_ms: s.prebufferMs,
      reduced_motion: s.reducedMotion,
      window: null,
      monitor_enabled: s.monitorEnabled,
      monitor_device: s.monitorDevice,
      monitor_gain_db: s.monitorGainDb,
      monitor_muted: s.monitorMuted,
      motion_mode: s.motionMode,
      touch_return_s: s.touchReturnS,
      dog: s.dog,
    };
    await api.configSave(cfg).catch(() => {});
  },
}));
