// Placeholder for a secret in the main window: shows only whether it is set and opens the
// capture-protected Secrets window to view or edit it.
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, SecretInfo } from "../api";
import { act } from "../store";

/** Live list of secret names + "is set" flags (never values). */
export function useSecrets(): SecretInfo[] {
  const [items, setItems] = useState<SecretInfo[]>([]);
  useEffect(() => {
    const load = () => api.secretsList().then((r) => setItems(r.items)).catch(() => {});
    load();
    const un = listen("secrets:changed", load);
    return () => { un.then((f) => f()); };
  }, []);
  return items;
}

export function SecretFields({ prefix, secrets }: { prefix: string; secrets: SecretInfo[] }) {
  return (
    <>
      {secrets.filter((s) => s.id.startsWith(prefix)).map((s) => (
        <div key={s.id} className="prop-row">
          <span className="prop-label">{s.label}</span>
          <div className="prop-path">
            <span className={`secret-status ${s.set ? "set" : ""}`}>{s.set ? "●●●●●●●●  saved" : "Not set"}</span>
            <button onClick={() => act(() => api.secretsOpen(s.id))} title="Opens a window that never appears on stream or in recordings">
              🛡 Edit securely
            </button>
          </div>
        </div>
      ))}
    </>
  );
}
