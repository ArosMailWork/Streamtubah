// Settings → Alerts: per-type style editor, test buttons, Alert Box overlay, recent events.
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { AlertEvent, AlertSettings, AlertStyle, api, AppSettings } from "../api";
import { act, toast } from "../store";

const AMOUNT_KINDS = ["bits", "superchat", "donation", "patron"];
const IMAGE_EXT = ["png", "gif", "jpg", "jpeg", "webp", "apng", "webm", "mp4"];
const SOUND_EXT = ["mp3", "wav", "ogg"];

export function AlertsTab(props: { draft: AppSettings; saveNow: (s: AppSettings) => void; setDraft: (s: AppSettings) => void }) {
  const { draft } = props;
  const [kinds, setKinds] = useState<[string, string][]>([]);
  const [sel, setSel] = useState("follow");
  const [recent, setRecent] = useState<AlertEvent[]>([]);
  const url = `http://127.0.0.1:${draft.api.port}/overlay/alerts`;

  useEffect(() => {
    api.alertsKinds().then(setKinds);
    api.alertsRecent().then((r) => setRecent(r.reverse()));
    const un = listen<AlertEvent>("alert:event", (e) => setRecent((r) => [e.payload, ...r].slice(0, 50)));
    return () => { un.then((f) => f()); };
  }, []);

  const alerts: AlertSettings = draft.alerts;
  const style: AlertStyle | undefined = alerts.styles[sel];
  const withStyle = (kind: string, patch: Partial<AlertStyle>): AppSettings =>
    ({ ...draft, alerts: { ...alerts, styles: { ...alerts.styles, [kind]: { ...alerts.styles[kind], ...patch } } } });
  /** Applies immediately so "Test" and open overlays use the new look. */
  const save = (patch: Partial<AlertStyle>) => props.saveNow(withStyle(sel, patch));
  const edit = (patch: Partial<AlertStyle>) => props.setDraft(withStyle(sel, patch));
  const commit = () => props.saveNow(draft);

  const pick = async (exts: string[], field: "image" | "sound") => {
    const path = await open({ multiple: false, filters: [{ name: field === "image" ? "Images / videos" : "Sounds", extensions: exts }] });
    if (typeof path !== "string") return;
    const name = await act(() => api.alertsImportMedia(path));
    if (name) save({ [field]: name });
  };

  return (
    <div className="alerts-tab">
      <div className="prop-info">
        Alerts appear in the <b>Alert Box</b>: a transparent web overlay added to your scene as a Browser Source. Sounds play through it
        (shown in the mixer). Twitch subs, gifts, bits and raids, Kick subs/gifts/hosts and YouTube Super Chats/members work without
        signing in; Twitch follows need Twitch sign-in.
      </div>
      <div className="row">
        <button className="primary" onClick={async () => { if (await act(() => api.alertsAddToScene())) toast("Alert Box added to the scene"); }}>
          ＋ Add Alert Box to current scene
        </button>
        <code className="grow url">{url}</code>
        <button onClick={() => navigator.clipboard.writeText(url).then(() => toast("URL copied"))}>Copy</button>
        <button title="Skip the alert on screen" onClick={() => api.alertsSkip()}>Skip current</button>
      </div>

      <div className="alerts-editor">
        <div className="list alerts-kinds">
          {kinds.map(([k, label]) => (
            <div key={k} className={`list-row ${k === sel ? "active" : ""}`} onClick={() => setSel(k)}>
              <input type="checkbox" checked={!!alerts.styles[k]?.enabled} onClick={(e) => e.stopPropagation()}
                onChange={(e) => props.saveNow(withStyle(k, { enabled: e.target.checked }))} />
              <span className="grow">{label}</span>
              <button className="small" onClick={(e) => { e.stopPropagation(); act(() => api.alertsTest(k)); }}>Test</button>
            </div>
          ))}
        </div>

        {style && (
          <div className="alert-style">
            <div className="prop-row">
              <span className="prop-label">Text</span>
              <input value={style.template} onChange={(e) => edit({ template: e.target.value })} onBlur={commit} />
            </div>
            <div className="muted small">Placeholders: {"{name} {amount} {count} {message} {platform}"}</div>
            <div className="prop-row">
              <span className="prop-label">Image / GIF / video</span>
              <div className="prop-path">
                <span className="grow muted">{style.image || "Built-in animated icon"}</span>
                <button onClick={() => pick(IMAGE_EXT, "image")}>Choose…</button>
                <button disabled={!style.image} onClick={() => save({ image: "" })}>Default</button>
              </div>
            </div>
            <div className="prop-row">
              <span className="prop-label">Sound</span>
              <div className="prop-path">
                <select value={style.sound === "" || style.sound === "none" ? style.sound : "file"}
                  onChange={(e) => (e.target.value === "file" ? pick(SOUND_EXT, "sound") : save({ sound: e.target.value }))}>
                  <option value="">Built-in chime</option>
                  <option value="none">No sound</option>
                  <option value="file">{style.sound && style.sound !== "none" ? `Custom: ${style.sound}` : "Custom file…"}</option>
                </select>
                <input type="range" min={0} max={1} step={0.05} value={style.volume} title={`Volume ${Math.round(style.volume * 100)}%`}
                  onChange={(e) => edit({ volume: +e.target.value })} onMouseUp={commit} onKeyUp={commit} />
              </div>
            </div>
            <div className="prop-row">
              <span className="prop-label">Animation</span>
              <select value={style.animation} onChange={(e) => save({ animation: e.target.value })}>
                <option value="bounce">Bounce</option>
                <option value="zoom">Zoom</option>
                <option value="slide">Slide up</option>
                <option value="fade">Fade</option>
              </select>
            </div>
            <div className="prop-row">
              <span className="prop-label">Duration</span>
              <div className="prop-number">
                <input type="number" min={1.5} max={60} step={0.5} value={style.duration_ms / 1000}
                  onChange={(e) => edit({ duration_ms: Math.round(+e.target.value * 1000) })} onBlur={commit} />
                <span className="muted">seconds</span>
              </div>
            </div>
            <div className="prop-row">
              <span className="prop-label">Colors & size</span>
              <div className="row">
                <label className="muted small">Text <input type="color" value={style.text_color} onChange={(e) => save({ text_color: e.target.value })} /></label>
                <label className="muted small">Highlight <input type="color" value={style.accent_color} onChange={(e) => save({ accent_color: e.target.value })} /></label>
                <label className="num-field"><input type="number" min={16} max={160} value={style.font_size}
                  onChange={(e) => edit({ font_size: +e.target.value })} onBlur={commit} /><span className="muted small">px</span></label>
              </div>
            </div>
            <label className="prop-check">
              <input type="checkbox" checked={style.show_message} onChange={(e) => save({ show_message: e.target.checked })} />
              Show the viewer's message
            </label>
            {AMOUNT_KINDS.includes(sel) && (
              <div className="prop-row">
                <span className="prop-label">Minimum amount</span>
                <div className="prop-number">
                  <input type="number" min={0} value={style.min_amount}
                    onChange={(e) => edit({ min_amount: Math.max(0, +e.target.value) })} onBlur={commit} />
                  <span className="muted small">{sel === "bits" ? "bits" : "in the currency received"} · 0 = all</span>
                </div>
              </div>
            )}
            <div className="row">
              <button className="primary" onClick={() => act(() => api.alertsTest(sel))}>▶ Test this alert</button>
            </div>
          </div>
        )}
      </div>

      <h3>Recent events</h3>
      <div className="list alerts-recent">
        {recent.length === 0 && <div className="muted small">No events yet.</div>}
        {recent.map((e) => (
          <div key={e.id} className="list-row">
            <span className="badge">{e.platform}</span>
            <span className="grow">{e.name} · {kinds.find(([k]) => k === e.kind)?.[1] ?? e.kind}
              {e.amount_text ? ` · ${e.amount_text}` : e.count ? ` · ${e.count}` : ""}{e.test ? " (test)" : ""}</span>
            <button className="small" title="Show again" onClick={() => act(() => api.alertsReplay(e.id))}>Replay</button>
          </div>
        ))}
      </div>
    </div>
  );
}
