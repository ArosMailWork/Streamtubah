// Self-update UI: a top-bar button when a new version exists, and Settings → About & Updates.
import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, UpdateInfo } from "../api";
import { act, createStore, toast, useStore } from "../store";
import { Modal } from "./Modal";

export const updateStore = createStore<UpdateInfo | null>(null);
/** Version installed and waiting for a restart. */
const installedStore = createStore<string | null>(null);

/** Checks once at startup when enabled in settings (quietly: network errors are ignored). */
export function useStartupUpdateCheck() {
  useEffect(() => {
    const t = setTimeout(async () => {
      try {
        const s = await api.settingsGet();
        if (s.updates.auto_check) updateStore.set(await api.updateCheck());
      } catch { /* offline or no release yet */ }
    }, 4000);
    return () => clearTimeout(t);
  }, []);
}

function UpdateDialog({ info, live, onClose }: { info: UpdateInfo; live: boolean; onClose: () => void }) {
  const installed = useStore(installedStore);
  const [busy, setBusy] = useState(false);
  const install = async () => {
    setBusy(true);
    const v = await act(() => api.updateInstall());
    setBusy(false);
    if (v) installedStore.set(v);
  };
  return (
    <Modal title={`Streamtubah ${info.version}`} onClose={onClose}
      footer={<>
        <button onClick={onClose}>Later</button>
        {info.needs_full_package ? (
          <button className="primary" onClick={() => openUrl(info.full_url)}>Download full package</button>
        ) : installed ? (
          <button className="primary" disabled={live} title={live ? "Stop streaming/recording first" : undefined}
            onClick={() => act(() => api.updateRestart())}>Restart now</button>
        ) : (
          <button className="primary" disabled={busy} onClick={install}>{busy ? "Downloading…" : "Install update"}</button>
        )}
      </>}>
      <div className="muted small">You have {info.current}.</div>
      {info.notes && <div className="update-notes">{info.notes}</div>}
      {info.needs_full_package && (
        <div className="prop-info warning">
          This version also updates the bundled OBS engine, so it cannot be installed in place. Download the full package and
          extract it over your current folder (your settings and scenes are kept in AppData).
        </div>
      )}
      {installed && <div className="prop-info">Installed {installed}. It starts the next time you open Streamtubah, or restart now.</div>}
      <div className="muted small">Updates are verified with the project's signature before they are installed.</div>
    </Modal>
  );
}

/** Top-bar button, shown only when an update is available. */
export function UpdateButton({ live }: { live: boolean }) {
  const info = useStore(updateStore);
  const [open, setOpen] = useState(false);
  if (!info?.available) return null;
  return (
    <>
      <button className="update-btn" onClick={() => setOpen(true)}>⬆ Update {info.version}</button>
      {open && <UpdateDialog info={info} live={live} onClose={() => setOpen(false)} />}
    </>
  );
}

export function AboutTab({ autoCheck, setAutoCheck, lowMemory, setLowMemory, live }: {
  autoCheck: boolean; setAutoCheck: (v: boolean) => void; lowMemory: boolean; setLowMemory: (v: boolean) => void; live: boolean;
}) {
  const info = useStore(updateStore);
  const [busy, setBusy] = useState(false);
  const [open, setOpen] = useState(false);
  const check = async () => {
    setBusy(true);
    const r = await act(() => api.updateCheck());
    setBusy(false);
    if (r) {
      updateStore.set(r);
      if (!r.available) toast(`You have the latest version (${r.current})`);
    }
  };
  return (
    <>
      <div className="prop-row"><span className="prop-label">Version</span><span>{info?.current ?? "…"}</span></div>
      <label className="prop-check">
        <input type="checkbox" checked={autoCheck} onChange={(e) => setAutoCheck(e.target.checked)} />
        Check for updates when Streamtubah starts
      </label>
      <div className="row">
        <button disabled={busy} onClick={check}>{busy ? "Checking…" : "Check now"}</button>
        {info?.available && <button className="primary" onClick={() => setOpen(true)}>Update to {info.version}</button>}
      </div>
      <h3>Performance</h3>
      <label className="prop-check">
        <input type="checkbox" checked={lowMemory} onChange={(e) => setLowMemory(e.target.checked)} />
        Low-memory interface (draws the interface without the graphics card: about 75 MB less RAM, slightly less smooth
        animations; streaming and recording are not affected). Takes effect after a restart.
      </label>
      <div className="muted small">
        Updates come from GitHub Releases (ArosMailWork/Streamtubah) and only replace the app itself (a few MB).
        Streamtubah is free software under GPL-3.0 and uses OBS Studio's libobs.
      </div>
      {open && info && <UpdateDialog info={info} live={live} onClose={() => setOpen(false)} />}
    </>
  );
}
