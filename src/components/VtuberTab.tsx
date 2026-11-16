// Settings → VTuber mode, and the top-bar toggle.
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { api, AppSettings, VtuberStatus } from "../api";
import { act, toast } from "../store";

function useVtuberStatus() {
  const [st, setSt] = useState<VtuberStatus | null>(null);
  useEffect(() => {
    api.vtuberStatus().then(setSt);
    const un = listen<VtuberStatus>("vtuber:status", (e) => setSt(e.payload));
    return () => { un.then((f) => f()); };
  }, []);
  return st;
}

/** Top bar: shows whether VTuber mode is on and warns when an app uses the camera. */
export function VtuberToggle() {
  const st = useVtuberStatus();
  if (!st) return null;
  const blocked = st.users.filter((u) => !u.allowed);
  const toggle = () => act(async () => {
    const s = await api.settingsGet();
    await api.settingsSet({ ...s, vtuber: { ...s.vtuber, enabled: !s.vtuber.enabled } });
    toast(s.vtuber.enabled ? "VTuber mode off: cameras are shown again" : "VTuber mode on: real cameras are covered");
  });
  return (
    <button className={`vtuber-toggle ${st.enabled ? "on" : ""} ${blocked.length ? "warn" : ""}`} onClick={toggle}
      title={st.enabled
        ? blocked.length ? `Camera in use by: ${blocked.map((u) => u.name).join(", ")} (screen captures covered). Click to turn VTuber mode off.` : "VTuber mode is on: webcam sources are covered. Click to turn off."
        : "VTuber mode is off. Click to cover real cameras on stream."}>
      🎭 VTuber {st.enabled ? (blocked.length ? `⚠ ${blocked.length}` : "on") : "off"}
    </button>
  );
}

export function VtuberTab(props: { draft: AppSettings; saveNow: (s: AppSettings) => void }) {
  const { draft } = props;
  const v = draft.vtuber;
  const st = useVtuberStatus();
  const [newApp, setNewApp] = useState("");
  const set = (patch: Partial<AppSettings["vtuber"]>) => props.saveNow({ ...draft, vtuber: { ...v, ...patch } });
  const allow = (name: string) => {
    const entry = name.replace(/\.exe$/i, "").trim().toLowerCase();
    if (entry && !v.allowed_apps.some((a) => a.toLowerCase() === entry)) set({ allowed_apps: [...v.allowed_apps, entry] });
  };
  const pickCover = async () => {
    const path = await open({ multiple: false, filters: [{ name: "Images", extensions: ["png", "gif", "jpg", "jpeg", "webp"] }] });
    if (typeof path === "string") act(async () => set({ cover_image: await api.vtuberImportCover(path) }));
  };

  return (
    <>
      <div className="prop-info">
        Keeps your real face off the stream and recordings. Streamtubah never closes or blocks other apps; it only covers
        what it sends out.
      </div>
      <label className="prop-check">
        <input type="checkbox" checked={v.enabled} onChange={(e) => set({ enabled: e.target.checked })} />
        <b>VTuber mode</b>
      </label>
      <ul className="muted small">
        <li>Webcam sources (Video Capture Device) are always covered.</li>
        <li>When an app outside the list below uses your camera you get a warning, Window / Game Capture of that app is
          covered, and every Display Capture is covered until it stops using the camera.</li>
        <li>Browser Sources are covered if a web page inside Streamtubah opens the camera.</li>
      </ul>

      <h3>Cover image</h3>
      <div className="prop-row">
        <span className="prop-label">Image</span>
        <div className="prop-path">
          <input readOnly value={v.cover_image ? v.cover_image : "Default character (animated)"} />
          <button onClick={pickCover}>Import…</button>
          {v.cover_image && <button onClick={() => set({ cover_image: "" })}>Default</button>}
        </div>
      </div>

      <h3>Apps using the camera now</h3>
      {!v.enabled ? <div className="muted small">Turn VTuber mode on to watch the camera.</div>
        : !st || st.users.length === 0 ? <div className="muted small">No app is using the camera.</div>
        : st.users.map((u) => (
          <div key={u.app} className="row small">
            <span className="grow" title={u.app}>{u.allowed ? "✅" : "⚠"} {u.name}</span>
            {u.allowed ? <span className="muted">allowed (face tracking)</span>
              : <button onClick={() => allow(u.name)}>Allow this app</button>}
          </div>
        ))}

      <h3>Allowed apps (face tracking)</h3>
      <p className="muted small">These may use the camera without covering your screen captures: they show your avatar,
        not your face. A name matches any app whose file name contains it.</p>
      <div className="chips">
        {v.allowed_apps.map((a) => (
          <span key={a} className="badge">{a} <button className="icon small" title="Remove"
            onClick={() => set({ allowed_apps: v.allowed_apps.filter((x) => x !== a) })}>✕</button></span>
        ))}
      </div>
      <div className="prop-path">
        <input placeholder="App name, e.g. vtube studio" value={newApp} onChange={(e) => setNewApp(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter") { allow(newApp); setNewApp(""); } }} />
        <button disabled={!newApp.trim()} onClick={() => { allow(newApp); setNewApp(""); }}>Add</button>
      </div>
    </>
  );
}
