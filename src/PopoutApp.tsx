// Chat pop-out window: borderless, always on top, adjustable background opacity,
// click-through (toggled by a global hotkey because the window cannot be clicked then).
import { useEffect } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "./App.css";
import { api } from "./api";
import { ChatView } from "./components/ChatView";
import { popoutStore, toastStore, useBackendEvents, useStore } from "./store";

const OPACITY_KEY = "streamtubah.popout.opacity";

export default function PopoutApp() {
  useBackendEvents();
  const st = useStore(popoutStore);
  const toasts = useStore(toastStore);

  useEffect(() => {
    document.documentElement.classList.add("popout-root");
    const saved = localStorage.getItem(OPACITY_KEY);
    if (saved !== null) api.popoutSetOpacity(Number(saved)).catch(() => {});
  }, []);

  const setOpacity = (v: number) => {
    localStorage.setItem(OPACITY_KEY, String(v));
    api.popoutSetOpacity(v).catch(() => {});
  };
  const hotkey = st.hotkey.split("+").map((k) => k[0].toUpperCase() + k.slice(1)).join("+");

  return (
    <div className={`popout ${st.click_through ? "ghost" : ""}`} style={{ ["--popout-alpha" as string]: st.opacity }}>
      <div className="popout-bar" data-tauri-drag-region>
        <span className="brand small" data-tauri-drag-region>Chat</span>
        <label className="row small" title="Background opacity (text stays readable)">
          ◐
          <input type="range" min={0} max={1} step={0.05} value={st.opacity} onChange={(e) => setOpacity(+e.target.value)} />
        </label>
        <button className={`small ${st.click_through ? "on" : ""}`} title={`Let the mouse pass through to the window below. Toggle with ${hotkey}.`}
          onClick={() => api.popoutSetClickThrough(!st.click_through)}>
          Click-through
        </button>
        <span className="grow" data-tauri-drag-region />
        <button className="icon small" title="Close" onClick={() => getCurrentWindow().close()}>✕</button>
      </div>
      {st.click_through && <div className="popout-hint">Click-through on — press {hotkey} to interact</div>}
      <ChatView compact />
      <div className="toasts">{toasts.map((t) => <div key={t.id} className={`toast ${t.kind}`}>{t.text}</div>)}</div>
    </div>
  );
}
