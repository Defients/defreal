import { create } from "zustand";
import { api, onTelemetry } from "./ipc";
import type {
  AppConfig,
  ChainParams,
  DeviceInfo,
  PresetMeta,
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
  setManual: (d: number, az: number, face: number) => Promise<void>;
  releaseManual: () => Promise<void>;
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
      const [params, presets, cfg, active] = await Promise.all([
        api.paramsGet(),
        api.presetsList(),
        api.configLoad(),
        api.activePreset(),
      ]);
      set({
        params,
        presets,
        activePreset: active,
        inputDevice: cfg.input_device,
        outputDevice: cfg.output_device,
        blockMs: cfg.block_ms || 5,
        prebufferMs: cfg.prebuffer_ms || 12,
        reducedMotion: cfg.reduced_motion || false,
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

  setManual: async (d, az, face) => {
    await api.motionSetManual(d, az, face).catch(() => {});
  },

  releaseManual: async () => {
    await api.motionRelease().catch(() => {});
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
    };
    await api.configSave(cfg).catch(() => {});
  },
}));
