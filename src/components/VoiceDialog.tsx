// Voice Studio: a dedicated chain editor for one audio source.
// No built-in effects — it is a host for plug-ins: OBS audio filters, VST 2.x
// plug-ins, and the Audio Bridge that hands audio to any external app.
import { useEffect, useState } from "react";
import { api, ApiInfo, FilterInfo, StudioState, TypeInfo } from "../api";
import { act } from "../store";
import { Modal } from "./Modal";
import { Meter } from "./Panels";
import { PropertiesForm } from "./PropertiesForm";

/** Quick-add slots shown first; any other audio filter is available below. */
const SLOTS: { id: string; label: string; hint: string }[] = [
  { id: "streamtubah_audio_bridge", label: "External app (Audio Bridge)", hint: "Send this voice to another program over WebSocket and get it back processed" },
  { id: "vst_filter", label: "VST 2.x plug-in", hint: "Load a VST 2 voice changer / effect installed on this PC" },
  { id: "noise_suppress_filter_v2", label: "Noise suppression", hint: "RNNoise / NVIDIA background noise removal" },
  { id: "noise_gate_filter", label: "Noise gate", hint: "Silence the mic below a threshold" },
  { id: "compressor_filter", label: "Compressor", hint: "Even out loud and quiet speech" },
  { id: "basic_eq_filter", label: "3-band EQ", hint: "Bass / mid / treble" },
  { id: "gain_filter", label: "Gain", hint: "Boost or cut level" },
  { id: "limiter_filter", label: "Limiter", hint: "Prevent clipping" },
];

export function VoiceDialog(props: { state: StudioState; initial?: string; onClose: () => void }) {
  const sources = props.state.audio;
  const [source, setSource] = useState(props.initial ?? sources.find((a) => a.name === "Mic/Aux")?.name ?? sources[0]?.name ?? "");
  const [filters, setFilters] = useState<FilterInfo[]>([]);
  const [types, setTypes] = useState<TypeInfo[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [bypass, setBypass] = useState(false);
  const [apiInfo, setApiInfo] = useState<ApiInfo | null>(null);
  const info = sources.find((a) => a.name === source);
  const previewing = (info?.monitoring ?? 0) !== 0;

  const reload = async () => {
    if (!source) return;
    const list = await api.filters(source);
    setFilters(list);
    setSelected((cur) => (cur && list.some((f) => f.name === cur) ? cur : list[0]?.name ?? null));
  };

  useEffect(() => {
    api.apiInfo().then(setApiInfo);
  }, []);
  useEffect(() => {
    if (!source) return;
    reload();
    api.filterTypes(source).then(setTypes).catch(() => setTypes([]));
    api.voiceBypassed(source).then(setBypass).catch(() => setBypass(false));
  }, [source]);

  const run = async (fn: () => Promise<unknown>) => {
    await act(fn);
    await reload();
  };

  const add = async (t: TypeInfo) => {
    const name = await act(() => api.filterAdd(source, t.id, t.name));
    await reload();
    if (name) setSelected(name);
  };

  const available = (id: string) => types.find((t) => t.id === id);
  const others = types.filter((t) => !SLOTS.some((s) => s.id === t.id));
  const selectedInfo = filters.find((f) => f.name === selected);
  const bridgeChannel = selectedInfo?.type_id === "streamtubah_audio_bridge";

  if (!source) {
    return (
      <Modal title="Voice Studio" onClose={props.onClose}>
        <div className="muted">No audio source. Choose a microphone in Settings → Audio.</div>
      </Modal>
    );
  }

  return (
    <Modal title="Voice Studio" onClose={props.onClose} wide footer={<button className="primary" onClick={props.onClose}>Done</button>}>
      <div className="voice-top">
        <select value={source} onChange={(e) => setSource(e.target.value)}>
          {sources.map((a) => <option key={a.name} value={a.name}>{a.name}</option>)}
        </select>
        <div className="grow"><Meter name={source} /></div>
        {/* Preview toggles between "off" and "monitor + output", so the stream mix never changes.
            "Monitor only" (1) already removes the source from the stream; leave it alone. */}
        <button className={previewing ? "on" : ""} disabled={info?.monitoring === 1}
          title={info?.monitoring === 1 ? "Set to monitor-only in the mixer" : "Hear this source in your monitoring device. The stream is not affected."}
          onClick={() => act(() => api.audioSetMonitoring(source, previewing ? 0 : 2))}>
          {info?.monitoring === 1 ? "🎧 Monitor only" : previewing ? "🎧 Preview on" : "🎧 Preview"}
        </button>
        <button className={bypass ? "on" : ""} title="Temporarily disable every effect to compare"
          onClick={async () => {
            await act(() => api.voiceSetBypass(source, !bypass));
            setBypass(!bypass);
            await reload();
          }}>
          {bypass ? "Bypassed" : "Bypass"}
        </button>
      </div>
      {info?.bluetooth_mic && (
        <div className="prop-info warning">
          This is a Bluetooth headset microphone. While it is open, Windows switches the headset to hands-free mode
          (mono, 16 kHz) — what you hear and the captured desktop audio lose quality. Use a USB/built-in mic if you can.
        </div>
      )}

      <div className="split">
        <div className="split-list">
          <h3>Chain (top → bottom)</h3>
          <div className="list">
            {filters.map((f, i) => (
              <div key={f.name} className={`list-row ${selected === f.name ? "active" : ""}`} onClick={() => setSelected(f.name)}>
                <span className="muted small">{i + 1}</span>
                <input type="checkbox" checked={f.enabled} onClick={(e) => e.stopPropagation()}
                  onChange={(e) => run(() => api.filterSetEnabled(source, f.name, e.target.checked))} />
                <span className="grow">{f.name}</span>
              </div>
            ))}
            {filters.length === 0 && <div className="empty">Empty chain — add a slot below</div>}
          </div>
          {selected && (
            <div className="row">
              <button onClick={() => run(() => api.filterMove(source, selected, true))}>▲</button>
              <button onClick={() => run(() => api.filterMove(source, selected, false))}>▼</button>
              <button className="danger" onClick={() => run(() => api.filterRemove(source, selected))}>Remove</button>
            </div>
          )}
          <h3>Add slot</h3>
          <div className="slot-list">
            {SLOTS.filter((s) => available(s.id)).map((s) => (
              <button key={s.id} className="slot" title={s.hint} onClick={() => add(available(s.id)!)}>{s.label}</button>
            ))}
          </div>
          {others.length > 0 && (
            <select value="" onChange={(e) => { const t = available(e.target.value); if (t) add(t); }}>
              <option value="">More filters & plug-ins…</option>
              {others.map((t) => <option key={t.id} value={t.id}>{t.name}</option>)}
            </select>
          )}
        </div>

        <div className="split-detail">
          {selected ? (
            <>
              <PropertiesForm target={{ kind: "filter", source, filter: selected }} />
              {bridgeChannel && apiInfo && (
                <div className="prop-info">
                  <b>Connect an app:</b> any program can process this voice by connecting to
                  <code> ws://127.0.0.1:{apiInfo.port}/audio/&lt;channel&gt;?password=…</code>.
                  It receives 48 kHz stereo float32 blocks and sends processed blocks back.
                  Python example: <code>examples/audio-bridge/voice_fx.py</code>
                  <div className="row" style={{ marginTop: 6 }}>
                    <button onClick={() => act(() => api.secretsOpen("api.password"))}>
                      🛡 Show password (secure window)
                    </button>
                  </div>
                </div>
              )}
              {selectedInfo?.type_id === "vst_filter" && (
                <div className="prop-info">
                  VST editor windows are not supported yet; the plug-in runs with its default/saved parameters.
                </div>
              )}
            </>
          ) : (
            <div className="voice-help">
              <p>Build a voice chain from plug-ins. Effects run top to bottom on this source only.</p>
              <ul>
                <li><b>External app</b> — connect a voice changer, AI voice model or script via the Audio Bridge.</li>
                <li><b>VST 2.x</b> — use voice plug-ins installed on this PC.</li>
                <li><b>Virtual mic apps</b> (Voicemod, VoiceMeeter…) — pick their virtual device as the mic in Settings → Audio.</li>
                <li><b>Audio from another app</b> — add an "Application Audio Capture" source to a scene.</li>
              </ul>
              <p className="muted">Preview plays this source in your monitoring device only; the stream mix is unchanged.</p>
            </div>
          )}
        </div>
      </div>
    </Modal>
  );
}
