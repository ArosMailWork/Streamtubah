import { useEffect, useRef, useState } from "react";
import "./App.css";
import { ChatDrawer } from "./components/ChatDrawer";
import { StatusBar } from "./components/Controls";
import { SettingsDialog, SettingsTab } from "./components/SettingsDialog";
import { NewMachineDialog } from "./components/NewMachineDialog";
import { applyPreset, LayoutDrawer } from "./layout/LayoutDrawer";
import { LayoutView } from "./layout/LayoutView";
import { DragGhost } from "./layout/dock";
import { activePreset, addPanel, MODES, useLayouts, WIDGETS, widgetsIn } from "./layout/model";
import { studioStore, toastStore, useBackendEvents, useStore } from "./store";

const CHAT_OPEN_KEY = "streamtubah.chat.open";

function Toasts() {
  const toasts = useStore(toastStore);
  return (
    <div className="toasts">
      {toasts.map((t) => <div key={t.id} className={`toast ${t.kind}`}>{t.text}</div>)}
    </div>
  );
}

export default function App() {
  useBackendEvents();
  const state = useStore(studioStore);
  const { presets, active } = useLayouts();
  const preset = presets.find((p) => p.id === active) ?? presets[0];
  const [settingsTab, setSettingsTab] = useState<SettingsTab | null>(null);
  const [chatOpen, setChatOpenRaw] = useState(() => localStorage.getItem(CHAT_OPEN_KEY) === "1");
  const live = !!state && (state.outputs.streaming || state.outputs.recording || state.outputs.replay);
  const lastByMode = useRef<Record<string, string>>({});
  lastByMode.current[preset.mode] = preset.id;

  const setChatOpen = (open: boolean) => {
    localStorage.setItem(CHAT_OPEN_KEY, open ? "1" : "0");
    setChatOpenRaw(open);
  };

  // Bring the backend's Studio Mode in line with the saved layout once at startup.
  const synced = useRef(false);
  useEffect(() => {
    if (!state || synced.current) return;
    synced.current = true;
    if (state.studio_mode !== (activePreset().mode === "studio")) applyPreset(activePreset(), () => {});
  }, [state]);

  const switchMode = (mode: string) => {
    const id = lastByMode.current[mode];
    const target = presets.find((p) => p.id === id) ?? presets.find((p) => p.mode === mode);
    if (target) applyPreset(target, setChatOpen);
  };

  return (
    <div className="app">
      <header className="topbar">
        <span className="brand">Streamtubah</span>
        <div className="mode-switch">
          {MODES.map((m) => (
            <button key={m.id} className={preset.mode === m.id ? "active" : ""} title={m.hint} onClick={() => switchMode(m.id)}>
              {m.name}
            </button>
          ))}
        </div>
        <span className="muted small">{preset.name}</span>
        {(() => {
          const missing = WIDGETS.filter((w) => !widgetsIn(preset.root).includes(w.id) && (w.id !== "program" || preset.mode === "studio"));
          return missing.length > 0 && (
            <select className="add-panel" value="" title="Add a closed panel back to this layout"
              onChange={(e) => e.target.value && addPanel(preset.id, e.target.value as (typeof WIDGETS)[number]["id"])}>
              <option value="">＋ Panel</option>
              {missing.map((w) => <option key={w.id} value={w.id}>{w.icon} {w.name}</option>)}
            </select>
          );
        })()}
        <span className="grow" />
        <button onClick={() => setSettingsTab("stream")}>⚙ Settings</button>
      </header>
      <div className="main-row">
        <LayoutDrawer setChatOpen={setChatOpen} />
        <div className="gutter" />
        <main className="workspace">{state && <LayoutView preset={preset} state={state} />}</main>
        <ChatDrawer open={chatOpen} setOpen={setChatOpen} onOpenSettings={() => setSettingsTab("chat")} />
      </div>
      <StatusBar />
      {settingsTab && <SettingsDialog live={live} initialTab={settingsTab} onClose={() => setSettingsTab(null)} />}
      {state && !settingsTab && <NewMachineDialog live={live} />}
      <DragGhost />
      <Toasts />
    </div>
  );
}
