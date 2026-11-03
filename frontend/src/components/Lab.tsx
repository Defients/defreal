import React from "react";
import { api } from "../ipc";
import { useStore } from "../store";
import type { AbResult, RenderReport } from "../types";

export function LabTab() {
  const presets = useStore((s) => s.presets);
  const activePreset = useStore((s) => s.activePreset);
  const [presetName, setPresetName] = React.useState(activePreset);
  const [inputPath, setInputPath] = React.useState("");
  const [outputPath, setOutputPath] = React.useState("");
  const [abDir, setAbDir] = React.useState("");
  const [report, setReport] = React.useState<RenderReport | null>(null);
  const [ab, setAb] = React.useState<AbResult | null>(null);
  const [busy, setBusy] = React.useState<string | null>(null);
  const [err, setErr] = React.useState<string | null>(null);

  const run = async (which: "render" | "ab") => {
    setErr(null);
    setBusy(which);
    try {
      if (which === "render") {
        setReport(await api.renderFile(inputPath, outputPath, presetName));
      } else {
        setAb(await api.renderAb(inputPath, abDir, presetName));
      }
    } catch (e) {
      setErr(String(e));
    }
    setBusy(null);
  };

  return (
    <div className="grid grid-cols-1 lg:grid-cols-2 gap-3">
      <div className="panel p-4">
        <div className="panel-title mb-3">Offline render</div>
        <p className="text-[10px] text-faint mb-3 leading-relaxed">
          Render a WAV file through the current chain (or a chosen preset) — deterministic, no
          real-time devices involved. Great for previewing and testing.
        </p>
        <div className="space-y-2">
          <label className="flex flex-col gap-1">
            <span className="text-[10px] text-faint">Input WAV (mono or stereo, any common rate)</span>
            <input className="select mono" value={inputPath} onChange={(e) => setInputPath(e.target.value)} placeholder="C:\path\to\input.wav" />
          </label>
          <label className="flex flex-col gap-1">
            <span className="text-[10px] text-faint">Output WAV path</span>
            <input className="select mono" value={outputPath} onChange={(e) => setOutputPath(e.target.value)} placeholder="C:\path\to\output.wav" />
          </label>
          <label className="flex flex-col gap-1">
            <span className="text-[10px] text-faint">Preset</span>
            <select className="select" value={presetName} onChange={(e) => setPresetName(e.target.value)}>
              {presets.map((p) => (
                <option key={p.name} value={p.name}>{p.name}</option>
              ))}
            </select>
          </label>
          <button
            className="btn btn-primary w-full"
            disabled={!inputPath || !outputPath || busy !== null}
            onClick={() => run("render")}
          >
            {busy === "render" ? "Rendering…" : "Render file"}
          </button>
          {report && (
            <div className="text-[10px] mono text-dim leading-relaxed border-t border-[var(--color-edge)] pt-2 mt-2">
              <div>in: {report.in_samples} samples @ {(report.in_rate / 1000).toFixed(1)}k · {report.in_rms_db.toFixed(1)} dB RMS</div>
              <div>out: {report.out_frames} frames · {report.out_rms_db.toFixed(1)} dB RMS · peak {report.out_peak_db.toFixed(1)} dB</div>
              <div>level match: {report.level_match_db.toFixed(2)} dB</div>
              <div className="text-[var(--color-cyan)]">→ {report.output_path}</div>
            </div>
          )}
        </div>
      </div>

      <div className="panel p-4">
        <div className="panel-title mb-3">A / B comparison</div>
        <p className="text-[10px] text-faint mb-3 leading-relaxed">
          Writes three files to a folder: <span className="mono">a_original.wav</span>,{" "}
          <span className="mono">b_processed.wav</span>, and a loudness-matched{" "}
          <span className="mono">b_processed_matched.wav</span> for honest comparison.
        </p>
        <div className="space-y-2">
          <label className="flex flex-col gap-1">
            <span className="text-[10px] text-faint">Output folder</span>
            <input className="select mono" value={abDir} onChange={(e) => setAbDir(e.target.value)} placeholder="C:\path\to\ab_folder" />
          </label>
          <button
            className="btn btn-primary w-full"
            disabled={!inputPath || !abDir || busy !== null}
            onClick={() => run("ab")}
          >
            {busy === "ab" ? "Rendering…" : "Generate A/B files"}
          </button>
          {ab && (
            <div className="text-[10px] mono text-dim leading-relaxed border-t border-[var(--color-edge)] pt-2 mt-2">
              <div>A: {ab.a_rms_db.toFixed(1)} dB RMS → {ab.a_path}</div>
              <div>B: {ab.b_rms_db.toFixed(1)} dB RMS → {ab.b_path}</div>
              <div>B matched ({ab.level_match_db.toFixed(2)} dB): {ab.b_matched_path}</div>
              <div>{ab.frames} frames</div>
            </div>
          )}
        </div>
      </div>

      {err && (
        <div className="xl:col-span-2 text-[11px] px-3 py-2 rounded-md border border-[var(--color-red)] text-[var(--color-red)]">
          {err}
        </div>
      )}
    </div>
  );
}
