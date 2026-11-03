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
  const blockMs = useStore((s) => s.blockMs);
  const prebufferMs = useStore((s) => s.prebufferMs);
  const persistConfig = useStore((s) => s.persistConfig);
  const running = useStore((s) => s.running);
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
              key={d.name}
              name={d.name}
              sub={`${d.default_channels}ch @ ${(d.default_rate / 1000).toFixed(1)}k${d.is_default ? " · default" : ""}`}
              selected={inputDevice === d.name}
              onSelect={() => {
                setInput(inputDevice === d.name ? null : d.name);
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
              key={d.name}
              name={d.name}
              sub={`${d.default_channels}ch @ ${(d.default_rate / 1000).toFixed(1)}k${d.is_default ? " · default" : ""}`}
              selected={outputDevice === d.name}
              onSelect={() => {
                setOutput(outputDevice === d.name ? null : d.name);
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

      <div className="panel p-4 xl:col-span-2">
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
