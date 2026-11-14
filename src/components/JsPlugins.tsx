// Settings → Plugins → JavaScript plugins: list, enable (with a warning), show window, reload, log.
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, JsPlugin, JsPluginLog } from "../api";
import { act } from "../store";
import { Modal } from "./Modal";

const PERMISSION_TEXT: Record<string, string> = {
  scenes: "Switch, create and remove scenes; show/hide sources in scenes",
  inputs: "Create, change and remove sources, volume, mute and filters",
  outputs: "Start and stop streaming, recording, replay buffer and virtual camera",
  collections: "Switch and create scene collections and profiles",
  audio: "Send audio into Plugin Audio sources and process audio through Audio Bridge filters",
  mic: "Use your microphone",
};

export function JsPluginsSection() {
  const [data, setData] = useState<{ folder: string; plugins: JsPlugin[] } | null>(null);
  const [confirm, setConfirm] = useState<JsPlugin | null>(null);
  const [logFor, setLogFor] = useState<string | null>(null);
  const refresh = () => api.jsPluginsList().then(setData);
  useEffect(() => {
    refresh();
    const un = listen("jsplugins:changed", refresh);
    return () => { un.then((f) => f()); };
  }, []);

  const enable = (p: JsPlugin, on: boolean) => {
    if (on) setConfirm(p);
    else act(() => api.jsPluginSetEnabled(p.id, false));
  };

  return (
    <>
      <h3>JavaScript plugins</h3>
      <p className="muted">
        Your own plugins written in JavaScript: one folder per plugin with a <code>manifest.json</code> (see
        <code> docs/js-plugins.md</code>). Each runs in its own hidden window and can open it to show its own interface.
      </p>
      <div className="prop-row">
        <span className="prop-label">Folder</span>
        <div className="prop-path">
          <input readOnly value={data?.folder ?? ""} />
          <button onClick={() => act(() => api.jsPluginsOpenFolder())}>Open</button>
          <button onClick={refresh}>Refresh</button>
        </div>
      </div>
      {data && data.plugins.length === 0 && <div className="muted small">No plugins in the folder yet.</div>}
      <div className="list">
        {data?.plugins.map((p) => (
          <div key={p.id} className="list-row js-plugin">
            <label className="prop-check grow" title={p.description}>
              <input type="checkbox" checked={p.enabled} disabled={!!p.manifest_error} onChange={(e) => enable(p, e.target.checked)} />
              <span>
                <b>{p.name}</b> {p.version && <span className="muted small">v{p.version}</span>}
                <span className="muted small"> · {p.permissions.length ? p.permissions.join(", ") : "no permissions"}</span>
                {p.running && <span className="badge active"> running</span>}
                {(p.manifest_error || p.error) && <div className="error small">{p.manifest_error || p.error}</div>}
              </span>
            </label>
            <button disabled={!p.running} onClick={() => act(() => api.jsPluginShow(p.id, true))}>Window</button>
            <button disabled={!p.enabled} onClick={() => act(() => api.jsPluginReload(p.id))}>Reload</button>
            <button onClick={() => setLogFor(p.id)}>Log</button>
          </div>
        ))}
      </div>

      {confirm && (
        <Modal title={`Enable "${confirm.name}"?`} onClose={() => setConfirm(null)}
          footer={
            <>
              <button onClick={() => setConfirm(null)}>Cancel</button>
              <button className="danger" onClick={() => { const id = confirm.id; setConfirm(null); act(() => api.jsPluginSetEnabled(id, true)); }}>
                I trust this plugin — enable
              </button>
            </>
          }>
          <div className="prop-info error">
            ⚠ Plugins are code that runs on this PC. Only enable plugins you wrote yourself or got from someone you trust.
            A plugin can reach the internet and, with the permissions below, control your stream and use your audio.
          </div>
          {confirm.description && <p>{confirm.description}</p>}
          <h4>It asks for:</h4>
          {confirm.permissions.length === 0 ? <p className="muted">No permissions (it can only show its own window and log).</p> : (
            <ul>{confirm.permissions.map((p) => <li key={p}><b>{p}</b> — {PERMISSION_TEXT[p] ?? p}</li>)}</ul>
          )}
          <p className="muted small">If a later version asks for more, it stops until you enable it again.</p>
        </Modal>
      )}
      {logFor && <PluginLog id={logFor} onClose={() => setLogFor(null)} />}
    </>
  );
}

function PluginLog(props: { id: string; onClose: () => void }) {
  const [lines, setLines] = useState<JsPluginLog[]>([]);
  useEffect(() => {
    api.jsPluginLogs(props.id).then(setLines);
    const un = listen<JsPluginLog>("jsplugins:log", (e) => {
      if (e.payload.id === props.id) setLines((l) => [...l.slice(-299), e.payload]);
    });
    return () => { un.then((f) => f()); };
  }, [props.id]);
  return (
    <Modal title={`Log — ${props.id}`} wide onClose={props.onClose}>
      <pre className="plugin-log">
        {lines.length === 0 ? "(empty)" : lines.map((l) => `${new Date(l.ts).toLocaleTimeString()} ${l.level.toUpperCase().padEnd(5)} ${l.text}`).join("\n")}
      </pre>
    </Modal>
  );
}
