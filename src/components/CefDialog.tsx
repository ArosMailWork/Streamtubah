// Browser component (Chromium for Browser Sources): download dialog + status row for Settings.
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { api, CefStatus, errorText } from "../api";
import { act, cefDialogStore, toast, useStore } from "../store";
import { Modal } from "./Modal";

function useCefStatus() {
  const [st, setSt] = useState<CefStatus | null>(null);
  useEffect(() => {
    api.cefStatus().then(setSt);
    const un = listen<CefStatus>("cef:status", (e) => setSt(e.payload));
    return () => { un.then((f) => f()); };
  }, []);
  return st;
}

function install(path?: string) {
  // Not through act(): a failure is shown in the dialog itself.
  api.cefInstall(path ?? null).catch((e) => toast(errorText(e), "error"));
}

async function installFromFile() {
  const path = await open({ multiple: false, filters: [{ name: "Browser component", extensions: ["zip"] }] });
  if (typeof path === "string") install(path);
}

function Actions({ st }: { st: CefStatus }) {
  if (st.restart_needed) {
    return <button className="primary" onClick={() => act(() => api.updateRestart())}>Restart now</button>;
  }
  return (
    <>
      <button disabled={st.busy} onClick={installFromFile} title="Use Streamtubah-cef-*.zip downloaded by hand (with its .json next to it)">Install from file…</button>
      <button className="primary" disabled={st.busy} onClick={() => install()}>{st.busy ? "Working…" : "Download (~110 MB)"}</button>
    </>
  );
}

/** Opened when something needs a Browser Source and the component is missing. */
export function CefDialog() {
  const show = useStore(cefDialogStore);
  const st = useCefStatus();
  if (!show || !st) return null;
  const close = () => cefDialogStore.set(false);
  return (
    <Modal title="Browser component needed" onClose={close}
      footer={<><button onClick={close}>Later</button><Actions st={st} /></>}>
      <p>
        Browser Sources (Alert Box, chat overlay, widgets, any web page on stream) run on a built-in Chromium. To keep
        the download small, Streamtubah fetches it the first time you need it: about 110 MB to download, 240 MB on disk.
      </p>
      <p className="muted small">
        It comes from the official Streamtubah release on GitHub and is checked (checksum + signature) before it is
        installed. Streamtubah restarts once to load it.
      </p>
      {st.state && <div className={`prop-info small ${st.state.startsWith("error") ? "error" : ""}`}>{st.state}</div>}
    </Modal>
  );
}

/** Settings → About: component state. */
export function CefStatusRow() {
  const st = useCefStatus();
  if (!st) return null;
  const text = st.loaded ? (st.bundled ? "built in" : "installed") : st.restart_needed ? "installed, restart needed" : "not installed";
  return (
    <div className="prop-row">
      <span className="prop-label">Browser component</span>
      <div className="prop-path">
        <span className="grow">{text}{st.state && !st.loaded && !st.restart_needed ? ` · ${st.state}` : ""}</span>
        {!st.loaded && <Actions st={st} />}
      </div>
    </div>
  );
}
