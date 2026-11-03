import React from "react";
import { useStore } from "../store";

export function PresetsTab() {
  const presets = useStore((s) => s.presets);
  const activePreset = useStore((s) => s.activePreset);
  const dirty = useStore((s) => s.dirty);
  const loadPreset = useStore((s) => s.loadPreset);
  const savePresetAs = useStore((s) => s.savePresetAs);
  const deletePreset = useStore((s) => s.deletePreset);
  const duplicatePreset = useStore((s) => s.duplicatePreset);
  const renamePreset = useStore((s) => s.renamePreset);
  const importPreset = useStore((s) => s.importPreset);
  const exportPreset = useStore((s) => s.exportPreset);
  const resetFactory = useStore((s) => s.resetFactory);
  const persistConfig = useStore((s) => s.persistConfig);
  const [newName, setNewName] = React.useState("");
  const [renaming, setRenaming] = React.useState<string | null>(null);
  const [renameText, setRenameText] = React.useState("");

  const filePick = (title: string): string | null => {
    // Tauri dialog plugin isn't installed; use a plain prompt for the path.
    return window.prompt(title) ?? null;
  };

  return (
    <div className="grid grid-cols-1 lg:grid-cols-3 gap-3">
      <div className="panel p-4 lg:col-span-2">
        <div className="panel-title mb-3">Presets</div>
        <div className="space-y-1.5">
          {presets.map((p) => (
            <div
              key={p.name}
              className={`flex items-center gap-3 px-3 py-2 rounded-md border ${
                p.name === activePreset
                  ? "border-[var(--color-violet-dim)] bg-[rgba(139,124,246,0.07)]"
                  : "border-[var(--color-edge)] bg-[var(--color-panel-2)]"
              }`}
            >
              <div className="flex-1 min-w-0">
                {renaming === p.name ? (
                  <input
                    autoFocus
                    className="bg-transparent border-b border-[var(--color-cyan)] text-[12px] outline-none w-full"
                    value={renameText}
                    onChange={(e) => setRenameText(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" && renameText.trim()) {
                        renamePreset(p.name, renameText.trim());
                        setRenaming(null);
                      }
                      if (e.key === "Escape") setRenaming(null);
                    }}
                  />
                ) : (
                  <>
                    <span className="text-[12px]">{p.name}</span>
                    {p.factory && <span className="badge ml-2">factory</span>}
                    {p.name === activePreset && dirty && (
                      <span className="badge warn ml-2">modified</span>
                    )}
                    <div className="text-[10px] text-faint truncate">{p.description}</div>
                  </>
                )}
              </div>
              {p.name !== activePreset && (
                <button
                  className="btn text-[10px]"
                  onClick={async () => {
                    await loadPreset(p.name);
                    persistConfig();
                  }}
                >
                  Load
                </button>
              )}
              {!p.factory && (
                <>
                  <button className="btn text-[10px]" onClick={() => { setRenaming(p.name); setRenameText(p.name); }}>
                    Rename
                  </button>
                  <button
                    className="btn text-[10px]"
                    onClick={() => {
                      const copy = window.prompt("Duplicate as:", `${p.name} Copy`);
                      if (copy) duplicatePreset(p.name, copy);
                    }}
                  >
                    Dup
                  </button>
                  <button className="btn text-[10px] btn-danger" onClick={() => deletePreset(p.name)}>
                    Delete
                  </button>
                </>
              )}
              {p.factory && (
                <button
                  className="btn text-[10px]"
                  onClick={() => {
                    const copy = window.prompt("Duplicate as:", `${p.name} Custom`);
                    if (copy) duplicatePreset(p.name, copy);
                  }}
                >
                  Dup
                </button>
              )}
            </div>
          ))}
        </div>
      </div>

      <div className="space-y-3">
        <div className="panel p-4">
          <div className="panel-title mb-3">Save current settings</div>
          <input
            className="select mb-2"
            placeholder="Preset name…"
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
          />
          <button
            className="btn btn-primary w-full"
            disabled={!newName.trim()}
            onClick={async () => {
              await savePresetAs(newName.trim());
              setNewName("");
              persistConfig();
            }}
          >
            Save preset
          </button>
          <p className="text-[10px] text-faint mt-2">
            Saves every module parameter + realism position. Presets persist across restarts.
          </p>
        </div>

        <div className="panel p-4">
          <div className="panel-title mb-3">Import / export</div>
          <div className="flex flex-col gap-2">
            <button
              className="btn"
              onClick={() => {
                const path = filePick("Path to a DefReal .json preset file:");
                if (path) importPreset(path);
              }}
            >
              Import .json preset
            </button>
            <button
              className="btn"
              onClick={() => {
                const path = filePick(`Export "${activePreset}" to path:`);
                if (path) exportPreset(activePreset, path);
              }}
            >
              Export active preset
            </button>
            <button
              className="btn btn-danger"
              onClick={() => {
                if (window.confirm("Reset all factory presets to defaults?")) resetFactory();
              }}
            >
              Reset factory presets
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
