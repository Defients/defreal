import React, { useEffect } from "react";
import { useStore } from "./store";
import { PerformTab } from "./components/Perform";
import { RoutingTab } from "./components/Routing";
import { PresetsTab } from "./components/Presets";
import { LabTab } from "./components/Lab";
import { DiagnosticsTab } from "./components/Diagnostics";

const TABS = [
  { id: "perform", label: "PERFORM" },
  { id: "routing", label: "ROUTING" },
  { id: "presets", label: "PRESETS" },
  { id: "lab", label: "LAB" },
  { id: "diagnostics", label: "DIAGNOSTICS" },
] as const;

export default function App() {
  const init = useStore((s) => s.init);
  const tab = useStore((s) => s.activeTab);
  const setTab = useStore((s) => s.setTab);
  const running = useStore((s) => s.running);
  const startEngine = useStore((s) => s.startEngine);
  const stopEngine = useStore((s) => s.stopEngine);
  const params = useStore((s) => s.params);
  const setParam = useStore((s) => s.setParam);
  const activePreset = useStore((s) => s.activePreset);
  const inputDevice = useStore((s) => s.inputDevice);
  const outputDevice = useStore((s) => s.outputDevice);
  const engineError = useStore((s) => s.engineError);
  const feedbackWarning = useStore((s) => s.feedbackWarning);
  const toast = useStore((s) => s.toast);
  const setToast = useStore((s) => s.setToast);
  const t = useStore((s) => s.telemetry);

  useEffect(() => {
    init();
  }, [init]);

  // auto-clear toast
  useEffect(() => {
    if (!toast) return;
    const id = setTimeout(() => setToast(null), 4000);
    return () => clearTimeout(id);
  }, [toast, setToast]);

  const canStart = inputDevice !== null && outputDevice !== null && !running;

  return (
    <div className="h-full flex flex-col">
      {/* top bar */}
      <header className="flex items-center gap-4 px-4 py-3 border-b border-[var(--color-edge)] bg-[var(--color-panel)]">
        <div className="flex items-baseline gap-2">
          <span className="text-[15px] font-bold tracking-[0.25em]">
            <span className="text-[var(--color-text)]">DEF</span>
            <span className="text-[var(--color-cyan)]">REAL</span>
          </span>
          <span className="text-[9px] tracking-[0.15em] text-faint hidden sm:inline border-l border-[var(--color-edge-lit)] pl-2 ml-1">
            ARTIFICIAL VOICE. NATURAL PRESENCE.
          </span>
        </div>

        <div className="flex-1" />

        <span className={`badge ${running ? "live" : ""}`}>
          {running ? "● LIVE" : "IDLE"}
        </span>
        <span className="badge">{activePreset}</span>
        {t && running && (
          <span className="badge mono">{t.est_latency_ms.toFixed(0)}ms</span>
        )}

        <button
          className={`btn ${params?.bypass ? "btn-active" : ""} ${running ? "" : "opacity-40"}`}
          disabled={!running}
          onClick={() => setParam("bypass", !params?.bypass)}
          title="Instant bypass — input passes through untouched"
        >
          BYPASS
        </button>
        <button
          className={`btn ${running ? "btn-danger" : "btn-primary"}`}
          disabled={!running && !canStart}
          onClick={() => (running ? stopEngine() : startEngine())}
          title={
            canStart
              ? `Route "${inputDevice}" → "${outputDevice}"`
              : running
                ? "Stop processing"
                : "Select input + output first (ROUTING tab)"
          }
        >
          {running ? "STOP" : "START"}
        </button>
      </header>

      {/* feedback warning */}
      {feedbackWarning && (
        <div className="px-4 py-2 bg-[rgba(232,176,74,0.08)] border-b border-[var(--color-amber)] flex items-center gap-3">
          <span className="text-[11px] text-[var(--color-amber)] flex-1">{feedbackWarning}</span>
          <button className="btn text-[10px]" onClick={() => startEngine(true)}>
            Start anyway
          </button>
          <button className="btn text-[10px]" onClick={() => useStore.setState({ feedbackWarning: null })}>
            Cancel
          </button>
        </div>
      )}

      {/* engine error */}
      {engineError && (
        <div className="px-4 py-2 bg-[rgba(224,86,86,0.08)] border-b border-[var(--color-red)]">
          <span className="text-[11px] text-[var(--color-red)]">{engineError}</span>
        </div>
      )}

      {/* tabs — active tab merges into the content surface below */}
      <nav className="flex gap-1 px-4 pt-3 -mb-px relative z-10">
        {TABS.map((t) => (
          <button
            key={t.id}
            onClick={() => setTab(t.id)}
            className={`px-4 py-1.5 text-[10px] tracking-[0.15em] rounded-t-md border border-b-0 transition-colors ${
              tab === t.id
                ? "border-[var(--color-edge-lit)] bg-[var(--color-void)] text-[var(--color-cyan)]"
                : "border-transparent text-faint hover:text-dim"
            }`}
          >
            {t.label}
          </button>
        ))}
      </nav>

      {/* content */}
      <main className="flex-1 overflow-y-auto px-4 py-4 border-t border-[var(--color-edge)]">
        <div className="max-w-[1600px] mx-auto w-full">
          {tab === "perform" && <PerformTab />}
          {tab === "routing" && <RoutingTab />}
          {tab === "presets" && <PresetsTab />}
          {tab === "lab" && <LabTab />}
          {tab === "diagnostics" && <DiagnosticsTab />}
        </div>
      </main>

      {/* footer */}
      <footer className="px-4 py-1.5 border-t border-[var(--color-edge)] flex justify-between text-[9px] text-faint">
        <span>Perfectly Imperfect Audio</span>
        <span className="mono">
          {inputDevice ?? "no input"} → {outputDevice ?? "no output"}
        </span>
      </footer>

      {/* toast */}
      {toast && (
        <div
          className={`toast fixed bottom-10 right-4 px-4 py-2 rounded-md text-[11px] border ${
            toast.kind === "error"
              ? "border-[var(--color-red)] text-[var(--color-red)] bg-[#1a0d0d]"
              : "border-[var(--color-cyan-dim)] text-[var(--color-cyan)] bg-[#0d1a19]"
          }`}
        >
          {toast.text}
        </div>
      )}
    </div>
  );
}
