import { useStore } from "../store";
import { api } from "../ipc";
import type { DogActivity, DogRoute, DogSize, DogTemperament } from "../types";

function Seg<T extends string>(props: {
  label: string;
  value: T;
  options: readonly T[];
  onChange: (v: T) => void;
  disabled?: boolean;
}) {
  return (
    <label className="flex flex-col gap-1">
      <span className="text-[10px] text-faint">{props.label}</span>
      <div className="flex gap-1">
        {props.options.map((o) => (
          <button
            key={o}
            disabled={props.disabled}
            onClick={() => props.onChange(o)}
            className={`btn text-[10px] flex-1 ${props.value === o ? "btn-active" : ""}`}
          >
            {o}
          </button>
        ))}
      </div>
    </label>
  );
}

export function DogPanel() {
  const dog = useStore((s) => s.dog);
  const t = useStore((s) => s.telemetry);
  const setDog = useStore((s) => s.setDog);
  const dogCmd = useStore((s) => s.dogCmd);
  const monitorEnabled = useStore((s) => s.monitorEnabled);

  const stateLabel = t?.dog_enabled ? t.dog_state : dog.enabled ? "starting" : "off";

  return (
    <div className="panel p-4">
      <div className="flex items-center justify-between mb-3">
        <div className="panel-title flex items-center gap-2">
          <span aria-hidden>🐾</span> DefDog
          <span className="text-[9px] font-mono text-faint normal-case">
            {stateLabel}
            {t?.dog_enabled && t.dog_last_event ? ` · ${t.dog_last_event}` : ""}
          </span>
        </div>
        <label className="flex items-center gap-1.5 text-[10px] text-dim">
          <input
            type="checkbox"
            checked={dog.enabled}
            onChange={(e) => setDog({ enabled: e.target.checked })}
          />
          Enable
        </label>
      </div>

      <div className="text-[9px] text-faint mb-3 leading-relaxed">
        An independent ambient presence in the same room — its own position,
        movement and sparse sound events. The most believable dog is a quiet
        one: it will often be silent. All sounds are procedurally synthesized
        (see docs); barking is off until you allow it.
      </div>

      <div className="grid grid-cols-1 gap-3">
        <Seg<DogSize>
          label="Size"
          value={dog.size}
          options={["Small", "Medium", "Large"] as const}
          onChange={(v) => setDog({ size: v })}
          disabled={!dog.enabled}
        />
        <Seg<DogTemperament>
          label="Temperament"
          value={dog.temperament}
          options={["Calm", "Curious", "Playful"] as const}
          onChange={(v) => setDog({ temperament: v })}
          disabled={!dog.enabled}
        />
        <Seg<DogActivity>
          label="Activity"
          value={dog.activity}
          options={["Rare", "Occasional", "Frequent"] as const}
          onChange={(v) => setDog({ activity: v })}
          disabled={!dog.enabled}
        />
        <Seg<DogRoute>
          label="Hear on"
          value={dog.route}
          options={["Both", "Main", "Monitor"] as const}
          onChange={(v) => setDog({ route: v })}
          disabled={!dog.enabled}
        />
      </div>

      <div className="flex flex-wrap items-center gap-x-4 gap-y-2 mt-3">
        <label className="flex-1 min-w-36 flex items-center gap-2 text-[10px] text-dim">
          Volume
          <input
            type="range"
            min={-48}
            max={-6}
            step={0.5}
            value={dog.volume_db}
            disabled={!dog.enabled}
            onChange={(e) => setDog({ volume_db: Number(e.target.value) })}
            className="flex-1"
          />
          <span className="font-mono w-14 text-right whitespace-nowrap">{dog.volume_db.toFixed(1)} dB</span>
        </label>
        <label
          className="flex items-center gap-1.5 text-[10px] text-dim"
          title="Off = never barks. Deliberate previews never bark either."
        >
          <input
            type="checkbox"
            checked={dog.allow_bark}
            disabled={!dog.enabled}
            onChange={(e) => setDog({ allow_bark: e.target.checked })}
          />
          Allow barking
        </label>
      </div>

      <div className="flex items-center gap-1.5 mt-3 pt-2 border-t border-[var(--color-edge)]">
        {(["come", "settle", "stay"] as const).map((c) => (
          <button
            key={c}
            className="btn text-[10px] capitalize"
            disabled={!dog.enabled}
            onClick={() => dogCmd(c)}
          >
            {c}
          </button>
        ))}
        <button
          className="btn text-[10px] ml-auto"
          disabled={!dog.enabled}
          title="Deliberate preview (never barks)"
          onClick={() => api.dogPreview().catch(() => {})}
        >
          Preview
        </button>
      </div>

      {dog.route === "Monitor" && !monitorEnabled && (
        <div className="mt-2 px-2 py-1.5 rounded border border-[var(--color-amber)] text-[9px] text-[var(--color-amber)]">
          Monitor-only routing selected but Direct Monitor is disabled — the dog won't be audible anywhere.
        </div>
      )}
      {t?.dog_enabled && (
        <div className="mt-2 flex justify-between text-[9px] font-mono text-faint">
          <span>
            {(t.dog_distance_m ?? 0).toFixed(2)} m · {(t.dog_azimuth_deg ?? 0).toFixed(0)}°
            {t.dog_moving ? " · moving" : ""}
          </span>
          <span>{t.dog_peak_db.toFixed(0)} dBpk</span>
        </div>
      )}
    </div>
  );
}
