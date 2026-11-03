import React from "react";
import { useStore } from "../store";
import { api } from "../ipc";

function DeviceRow(props: {
  name: string;
  sub: string;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      onClick={props.onSelect}
      className={`w-full text-left px-3 py-2 rounded-md border transition-colors ${
        props.selected
          ? "border-[var(--color-cyan-dim)] bg-[rgba(62,230,216,0.06)]"
          : "border-[var(--color-edge)] bg-[var(--color-panel-2)] hover:border-[var(--color-edge-lit)]"
      }`}
    >
      <div className="text-[12px] truncate">{props.name}</div>
      <div className="text-[10px] text-faint mono">{props.sub}</div>
    </button>
  );
}

export function RoutingTab() {
  const devices = useStore((s) => s.devices);
  const devicesError = useStore((s) => s.devicesError);
  const refreshDevices = useStore((s) => s.refreshDevices);
  const inputDevice = useStore((s) => s.inputDevice);
  const outputDevice = useStore((s) => s.outputDevice);
  const setInput = useStore((s) => s.setInput);
  const setOutput = useStore((s) => s.setOutput);
  const s = useStore();
  const blockMs = useStore((s) => s.blockMs);
  const prebufferMs = useStore((s) => s.prebufferMs);
  const persistConfig = useStore((s) => s.persistConfig);
  const running = useStore((s) => s.running);
  const telemetry = useStore((s) => s.telemetry);
  const [feedbackNote, setFeedbackNote] = React.useState<string | null>(null);

  React.useEffect(() => {
    if (inputDevice && outputDevice) {
      api.feedbackCheck(inputDevice, outputDevice).then(setFeedbackNote).catch(() => {});
    } else {
      setFeedbackNote(null);
    }
  }, [inputDevice, outputDevice]);

  return (
    <div className="grid grid-cols-1 xl:grid-cols-2 gap-3">
      <div className="panel p-4">
        <div className="flex items-center justify-between mb-3">
          <div className="panel-title">Capture from</div>
          <button className="btn text-[10px]" onClick={refreshDevices}>Refresh</button>
        </div>
        {devicesError && <div className="text-[11px] text-[var(--color-red)] mb-2">Device scan failed: {devicesError}</div>}
        <div className="space-y-1.5 max-h-80 overflow-y-auto pr-1">
          {devices.inputs.length === 0 && !devicesError && (
            <div className="text-[11px] text-faint">No capture devices found.</div>
          )}
          {devices.inputs.map((d) => (
            <DeviceRow
              key={d.id}
              name={d.name}
              sub={`${d.default_channels}ch @ ${(d.default_rate / 1000).toFixed(1)}k${d.is_default ? " · default" : ""}${d.id !== d.name ? ` · ${d.id}` : ""}`}
              selected={inputDevice === d.id}
              onSelect={() => {
                setInput(inputDevice === d.id ? null : d.id);
                persistConfig();
              }}
            />
          ))}
        </div>
      </div>

      <div className="panel p-4">
        <div className="panel-title mb-3">Send to</div>
        <div className="space-y-1.5 max-h-80 overflow-y-auto pr-1">
          {devices.outputs.map((d) => (
            <DeviceRow
              key={d.id}
              name={d.name}
              sub={`${d.default_channels}ch @ ${(d.default_rate / 1000).toFixed(1)}k${d.is_default ? " · default" : ""}${d.id !== d.name ? ` · ${d.id}` : ""}`}
              selected={outputDevice === d.id}
              onSelect={() => {
                setOutput(outputDevice === d.id ? null : d.id);
                persistConfig();
              }}
            />
          ))}
        </div>
        {feedbackNote && (
          <div className="mt-3 text-[11px] px-3 py-2 rounded-md border border-[var(--color-amber)] text-[var(--color-amber)]">
            {feedbackNote}
          </div>
        )}
      </div>

      {/* direct monitor */}
      <div className="panel p-4">
        <div className="flex items-center justify-between mb-3">
          <div className="panel-title">Direct Monitor</div>
          <div className="flex items-center gap-2">
            {telemetry && (
              <span className={`text-[9px] font-mono ${telemetry.monitor_state === "active" ? "text-[var(--color-cyan)]" : telemetry.monitor_state === "error" ? "text-[var(--color-warn)]" : "text-faint"}`}>
                {telemetry.monitor_state === "active"
                  ? `● ${(telemetry.monitor_rate / 1000).toFixed(1)}kHz u:${telemetry.monitor_underflows}`
                  : telemetry.monitor_state === "opening"
                    ? "… opening"
                    : telemetry.monitor_state === "error"
                      ? `⚠ ${telemetry.monitor_error ?? "error"}`
                      : "off"}
              </span>
            )}
            <label className="flex items-center gap-1.5 text-[10px] text-dim">
              <input
                type="checkbox"
                checked={s.monitorEnabled}
                onChange={(e) => s.setMonitor({ enabled: e.target.checked })}
                disabled={!s.monitorDevice && !s.monitorEnabled}
              />
              Enable
            </label>
          </div>
        </div>
        <div className="text-[9px] text-faint mb-2">
          Same processed signal, second independent output. Defaults to -18 dB
          and ramps in. Primary output is never affected by monitor failures.
        </div>
        <div className="flex flex-col gap-0.5 max-h-36 overflow-y-auto">
          {devices.outputs.map((d) => (
            <DeviceRow
              key={d.id}
              name={d.name}
              sub={`${d.default_channels}ch @ ${(d.default_rate / 1000).toFixed(1)}k${d.is_default ? " · default" : ""}${d.id !== d.name ? ` · ${d.id}` : ""}`}
              selected={s.monitorDevice === d.id}
              onSelect={() =>
                s.setMonitor({ device: s.monitorDevice === d.id ? null : d.id })
              }
            />
          ))}
        </div>
        <div className="flex items-center gap-4 mt-3">
          <label className="flex items-center gap-1.5 text-[10px] text-dim">
            <input
              type="checkbox"
              checked={s.monitorMuted}
              onChange={(e) => s.setMonitor({ muted: e.target.checked })}
              disabled={!s.monitorEnabled}
            />
            Mute
          </label>
          <label className="flex-1 flex items-center gap-2 text-[10px] text-dim">
            Gain
            <input
              type="range"
              min={-48}
              max={12}
              step={0.5}
              value={s.monitorGainDb}
              onChange={(e) => s.setMonitor({ gainDb: Number(e.target.value) })}
              disabled={!s.monitorEnabled}
              className="flex-1"
            />
            <span className="font-mono w-14 text-right">{s.monitorGainDb.toFixed(1)} dB</span>
          </label>
        </div>
        {s.monitorWarning && (
          <div className="mt-2 px-3 py-2 rounded-md border border-[var(--color-amber)] text-[10px] text-[var(--color-amber)]">
            ⚠ {s.monitorWarning}
          </div>
        )}
      </div>

      <div className="panel p-4">
        <div className="panel-title mb-3">Stream tuning</div>
        <div className="grid grid-cols-2 md:grid-cols-4 gap-4 text-[11px]">
          <label className="flex flex-col gap-1">
            <span className="text-faint">Block size</span>
            <select
              className="select"
              value={blockMs}
              disabled={running}
              onChange={(e) => {
                useStore.setState({ blockMs: parseInt(e.target.value) });
                persistConfig();
              }}
            >
              {[3, 5, 10, 20, 40].map((v) => (
                <option key={v} value={v}>{v} ms</option>
              ))}
            </select>
          </label>
          <label className="flex flex-col gap-1">
            <span className="text-faint">Pre-buffer</span>
            <select
              className="select"
              value={prebufferMs}
              disabled={running}
              onChange={(e) => {
                useStore.setState({ prebufferMs: parseInt(e.target.value) });
                persistConfig();
              }}
            >
              {[5, 12, 20, 40, 80].map((v) => (
                <option key={v} value={v}>{v} ms</option>
              ))}
            </select>
          </label>
          <div className="flex flex-col gap-1">
            <span className="text-faint">Internal rate</span>
            <span className="mono text-dim py-1.5">48 kHz</span>
          </div>
          <div className="flex flex-col gap-1">
            <span className="text-faint">DSP path</span>
            <span className="mono text-dim py-1.5">mono → stereo</span>
          </div>
        </div>
        <p className="mt-3 text-[10px] text-faint leading-relaxed">
          Smaller blocks = lower latency, higher CPU. If you hear crackles, raise pre-buffer.
          Routing into OBS/Discord: send output to a VB-Audio CABLE Input, then pick that cable's
          Output as the mic in the other app. For RVC: set RVC's output device to a cable, and pick
          that cable's Output as DefReal's input.
        </p>
      </div>
    </div>
  );
}
