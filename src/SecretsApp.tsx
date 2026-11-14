// Secrets window: excluded from screen capture by Windows, so streams, recordings and screen
// sharing never show it. Values are loaded only while the eye is open, and everything is
// masked again (and dropped from memory) as soon as the window loses focus.
import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import "./App.css";
import { api, errorText, SecretInfo } from "./api";

interface FieldState {
  value: string;
  revealed: boolean;
  /** Typed by the user and not saved yet. */
  dirty: boolean;
}

export default function SecretsApp() {
  const [items, setItems] = useState<SecretInfo[]>([]);
  const [lost, setLost] = useState(false);
  const [fields, setFields] = useState<Record<string, FieldState>>({});
  const [message, setMessage] = useState<{ text: string; error?: boolean } | null>(null);
  // Field to jump to: from the URL when the window was just created, later via "secrets:focus".
  const [focusId, setFocusId] = useState<string | null>(() => new URLSearchParams(location.search).get("focus"));
  const rowRefs = useRef<Record<string, HTMLDivElement | null>>({});

  const load = () => api.secretsList().then((r) => { setItems(r.items); setLost(r.lost); });
  const maskAll = () => setFields((f) => {
    const next: Record<string, FieldState> = {};
    // Revealed-but-unchanged values are dropped; unsaved typing stays, masked.
    for (const [id, s] of Object.entries(f)) if (s.dirty) next[id] = { ...s, revealed: false };
    return next;
  });

  useEffect(() => {
    document.documentElement.classList.add("secrets-root");
    load();
    const unFocus = listen<string>("secrets:focus", (e) => setFocusId(e.payload));
    const unChanged = listen("secrets:changed", () => load());
    window.addEventListener("blur", maskAll);
    document.addEventListener("visibilitychange", maskAll);
    return () => {
      unFocus.then((f) => f());
      unChanged.then((f) => f());
      window.removeEventListener("blur", maskAll);
      document.removeEventListener("visibilitychange", maskAll);
    };
  }, []);

  useEffect(() => {
    if (focusId && rowRefs.current[focusId]) {
      rowRefs.current[focusId]!.scrollIntoView({ block: "center" });
      rowRefs.current[focusId]!.querySelector("input")?.focus();
    }
  }, [focusId, items]);

  const state = (id: string): FieldState => fields[id] ?? { value: "", revealed: false, dirty: false };
  const setField = (id: string, patch: Partial<FieldState>) => setFields((f) => ({ ...f, [id]: { ...state(id), ...f[id], ...patch } }));

  const toggleEye = async (id: string) => {
    const s = state(id);
    if (s.revealed) {
      setField(id, { revealed: false, value: s.dirty ? s.value : "" });
      return;
    }
    if (s.dirty) {
      setField(id, { revealed: true });
      return;
    }
    try {
      setField(id, { revealed: true, value: await api.secretGet(id) });
    } catch (e) {
      setMessage({ text: errorText(e), error: true });
    }
  };
  const save = async (id: string, value: string) => {
    try {
      await api.secretSet(id, value);
      setFields((f) => { const n = { ...f }; delete n[id]; return n; });
      setMessage({ text: value ? "Saved" : "Cleared" });
      load();
    } catch (e) {
      setMessage({ text: errorText(e), error: true });
    }
  };

  const groups = [...new Set(items.map((i) => i.group))];
  return (
    <div className="secrets-app">
      <div className="secrets-banner">
        🛡 This window is hidden from screen capture: streams, recordings, screenshots and screen sharing show nothing here.
        Values stay masked unless you press 👁, and are hidden again when you leave this window.
      </div>
      {lost && (
        <div className="prop-info warning">
          Secrets saved on another PC or Windows user cannot be read here (they are encrypted for that user). Enter them again.
        </div>
      )}
      {message && <div className={`prop-info ${message.error ? "error" : ""}`}>{message.text}</div>}
      {groups.map((g) => (
        <section key={g} className="secrets-group">
          <h3>{g}</h3>
          {items.filter((i) => i.group === g).map((i) => {
            const s = state(i.id);
            return (
              <div key={i.id} className={`secret-row ${focusId === i.id ? "focus" : ""}`} ref={(el) => { rowRefs.current[i.id] = el; }}>
                <span className="prop-label">{i.label}</span>
                <input type={s.revealed ? "text" : "password"} autoComplete="off" spellCheck={false}
                  data-secret={i.id} value={s.value}
                  placeholder={i.set ? "●●●●●●●●  saved" : "Not set"}
                  onChange={(e) => setField(i.id, { value: e.target.value, dirty: true })}
                  onKeyDown={(e) => { if (e.key === "Enter" && s.dirty) save(i.id, s.value); }} />
                <button className="icon" title={s.revealed ? "Hide" : "Show"} data-eye={i.id} onClick={() => toggleEye(i.id)}>
                  {s.revealed ? "🙈" : "👁"}
                </button>
                <button className="primary" disabled={!s.dirty} data-save={i.id} onClick={() => save(i.id, s.value)}>Save</button>
                <button disabled={!i.set} title={i.id === "api.password" ? "Generate a new random password" : "Remove"}
                  data-clear={i.id} onClick={() => save(i.id, "")}>{i.id === "api.password" ? "New" : "Clear"}</button>
              </div>
            );
          })}
        </section>
      ))}
    </div>
  );
}
