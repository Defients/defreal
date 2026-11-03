import { useStore } from "../store";
import { Knob, Meter } from "./widgets";
import { Stage, MotionHistory } from "./Stage";
import { ModulePanels } from "./Modules";
import { DogPanel } from "./DogPanel";

export function PerformTab() {
  const params = useStore((s) => s.params);
  const setParam = useStore((s) => s.setParam);
  const t = useStore((s) => s.telemetry);
  const running = useStore((s) => s.running);

  if (!params) return <div className="text-faint text-sm p-8">Loading…</div>;

  return (
    <div className="grid grid-cols-1 xl:grid-cols-[300px_1fr] gap-3">
      {/* left: realism + meters */}
      <div className="space-y-3">
        <div className="panel p-4 flex flex-col items-center">
          <div className="panel-title mb-2">Natural Presence</div>
          <Knob
            value={params.realism}
            onChange={(v) => setParam("realism", v)}
            size={170}
            label="REALISM"
          />
          <div className="w-full mt-3 space-y-2">
            <Meter label="INPUT" rmsDb={t?.in_rms_db ?? -99} peakDb={t?.in_peak_db ?? -99} />
            <Meter label="OUTPUT" rmsDb={t?.out_rms_db ?? -99} peakDb={t?.out_peak_db ?? -99} />
          </div>
          <div className="w-full mt-3 space-y-1.5">
            <div className="flex justify-between text-[10px] text-faint">
              <span>MASTER</span>
              <input
                type="range"
                className="violet w-24"
                min={0}
                max={1}
                step={0.01}
                value={params.master_amount}
                onChange={(e) => setParam("master_amount", parseFloat(e.target.value))}
              />
            </div>
            <div className="flex justify-between text-[10px] text-faint">
              <span>IN GAIN</span>
              <input
                type="range"
                className="w-24"
                min={-24}
                max={24}
                step={0.5}
                value={params.input_gain_db}
                onChange={(e) => setParam("input_gain_db", parseFloat(e.target.value))}
              />
            </div>
            <div className="flex justify-between text-[10px] text-faint">
              <span>OUT GAIN</span>
              <input
                type="range"
                className="w-24"
                min={-24}
                max={24}
                step={0.5}
                value={params.output_gain_db}
                onChange={(e) => setParam("output_gain_db", parseFloat(e.target.value))}
              />
            </div>
          </div>
        </div>

        <div className="panel p-4">
          <div className="panel-title mb-2">Status</div>
          <div className="space-y-1 text-[11px]">
            <div className="flex justify-between">
              <span className="text-faint">Engine</span>
              <span className={running ? "text-[var(--color-cyan)]" : "text-dim"}>
                {t?.state ?? "idle"}
              </span>
            </div>
            <div className="flex justify-between">
              <span className="text-faint">Motion</span>
              <span className="mono text-dim">{t?.motion_state ?? "—"}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-faint">Distance</span>
              <span className="mono text-dim">{(t?.motion_distance_m ?? 0).toFixed(2)} m</span>
            </div>
            <div className="flex justify-between">
              <span className="text-faint">Latency</span>
              <span className="mono text-dim">
                {t ? `${t.est_latency_ms.toFixed(1)} ms` : "—"}
              </span>
            </div>
            <div className="flex justify-between">
              <span className="text-faint">Speech gate</span>
              <span className="mono text-dim">
                {t ? (t.speech_open ? "open" : "closed") : "—"}
              </span>
            </div>
          </div>
        </div>
      </div>

      {/* right: viz + modules */}
      <div className="space-y-3">
        <div className="grid grid-cols-1 xl:grid-cols-[1fr_320px] gap-3">
          <div className="panel p-4">
            <Stage />
            <div className="mt-2">
              <MotionHistory />
            </div>
          </div>
          <DogPanel />
        </div>
        <ModulePanels />
      </div>
    </div>
  );
}
