// Multistream: extra destinations in Settings → Stream, and live status in the Controls panel.
import { api, AppSettings, DestinationStatus, StreamTarget } from "../api";
import { act } from "../store";
import { PropertiesForm } from "./PropertiesForm";
import { SecretFields, useSecrets } from "./SecretField";

export const UPLOAD_KEY = "streamtubah.upload_mbps";
const AUDIO_KBPS_FALLBACK = 160;

const newId = () => Math.random().toString(36).slice(2, 10);

export function mainBitrate(s: AppSettings): number {
  const b = Number((s.output.video_encoder_settings as { bitrate?: unknown }).bitrate);
  return Number.isFinite(b) && b > 0 ? b : 6000;
}

/** Upload needed by all enabled destinations, in kbps. */
export function uploadNeeded(s: AppSettings): { kbps: number; count: number; own: number } {
  const audio = s.output.audio_bitrate || AUDIO_KBPS_FALLBACK;
  const main = mainBitrate(s);
  let kbps = s.stream.main_enabled ? main + audio : 0;
  let count = s.stream.main_enabled ? 1 : 0;
  let own = 0;
  for (const t of s.stream.targets.filter((t) => t.enabled)) {
    kbps += (t.own_encoder ? t.bitrate_kbps : main) + audio;
    count += 1;
    if (t.own_encoder) own += 1;
  }
  return { kbps, count, own };
}

/** Upload budget summary shown under the destination list. */
export function UploadSummary({ settings }: { settings: AppSettings }) {
  const { kbps, count, own } = uploadNeeded(settings);
  const measured = Number(localStorage.getItem(UPLOAD_KEY)) || 0;
  const safe = measured * 1000 * 0.75;
  const over = measured > 0 && kbps > safe;
  return (
    <div className={`prop-info ${over ? "warning" : ""}`}>
      {count} destination{count === 1 ? "" : "s"} · about <b>{(kbps / 1000).toFixed(1)} Mbit/s</b> upload needed
      {measured > 0 ? <> (your last speed test: {measured} Mbit/s{over ? " — too much, lower bitrates or disable a destination" : ", OK"})</>
        : <> — run Settings → Auto-Optimize to check your upload</>}
      {own > 0 && (
        <div className="small">
          {own} destination{own === 1 ? " uses" : "s use"} its own encoder: extra GPU work. Consumer NVIDIA cards run a limited
          number of encoders at once (older drivers: 3).
        </div>
      )}
    </div>
  );
}

export function DestinationsEditor(props: {
  draft: AppSettings;
  live: boolean;
  formKey: number;
  saveNow: (next: AppSettings) => void;
  setDraft: (next: AppSettings) => void;
}) {
  const { draft } = props;
  const targets = draft.stream.targets;
  const secrets = useSecrets();
  const update = (id: string, patch: Partial<StreamTarget>, immediate = false) => {
    const next = { ...draft, stream: { ...draft.stream, targets: targets.map((t) => (t.id === id ? { ...t, ...patch } : t)) } };
    if (immediate) props.saveNow(next);
    else props.setDraft(next);
  };
  const add = () => props.saveNow({
    ...draft,
    stream: {
      ...draft.stream,
      targets: [...targets, {
        id: newId(), name: `Destination ${targets.length + 2}`, enabled: true, service_type: "rtmp_common",
        service_settings: { service: "YouTube - RTMPS" }, own_encoder: false, bitrate_kbps: 6000,
      }],
    },
  });
  const remove = (id: string) => props.saveNow({ ...draft, stream: { ...draft.stream, targets: targets.filter((t) => t.id !== id) } });

  return (
    <>
      <h3>More destinations (multistream)</h3>
      <div className="muted small">
        Go live on several platforms at once, sent straight from this PC (no relay server). Destinations share the
        main encoder unless you give one its own quality. Toggling a destination while live starts or stops it.
      </div>
      {targets.map((t) => (
        <fieldset key={t.id} className="prop-group destination">
          <div className="row">
            <label className="prop-check">
              <input type="checkbox" checked={t.enabled} onChange={(e) => update(t.id, { enabled: e.target.checked }, true)} />
            </label>
            <input className="grow" value={t.name} onChange={(e) => update(t.id, { name: e.target.value })}
              onBlur={() => props.saveNow(draft)} />
            <button className="danger" disabled={props.live && t.enabled} title={props.live && t.enabled ? "Disable it first" : "Remove"}
              onClick={() => remove(t.id)}>Remove</button>
          </div>
          <div className="prop-row">
            <span className="prop-label">Service type</span>
            <select value={t.service_type} disabled={props.live && t.enabled}
              onChange={(e) => update(t.id, { service_type: e.target.value, service_settings: {} }, true)}>
              <option value="rtmp_common">Streaming platform (Twitch, YouTube, …)</option>
              <option value="rtmp_custom">Custom server (RTMP / SRT / RIST)</option>
              <option value="whip_custom">WHIP / WebRTC (low latency)</option>
            </select>
          </div>
          <PropertiesForm key={`t-${t.id}-${t.service_type}-${props.formKey}`} target={{ kind: "target_service", id: t.id }} />
          <SecretFields prefix={`target.${t.id}.`} secrets={secrets} />
          <div className="prop-row">
            <span className="prop-label">Video quality</span>
            <div className="row">
              <select value={t.own_encoder ? "own" : "shared"} disabled={props.live && t.enabled}
                onChange={(e) => update(t.id, { own_encoder: e.target.value === "own" }, true)}>
                <option value="shared">Same as main (no extra GPU/CPU)</option>
                <option value="own">Own bitrate (extra encoder)</option>
              </select>
              {t.own_encoder && (
                <label className="num-field">
                  <input type="number" min={500} max={50000} step={500} value={t.bitrate_kbps} disabled={props.live && t.enabled}
                    onChange={(e) => update(t.id, { bitrate_kbps: Math.max(500, +e.target.value) })} onBlur={() => props.saveNow(draft)} />
                  <span className="muted small">kbps</span>
                </label>
              )}
            </div>
          </div>
        </fieldset>
      ))}
      <button onClick={add}>＋ Add destination</button>
      <UploadSummary settings={draft} />
    </>
  );
}

/** Live status + on/off per destination, shown in the Controls panel when multistreaming. */
export function DestinationList({ destinations }: { destinations: DestinationStatus[] }) {
  if (destinations.length < 2) return null;
  const toggle = (d: DestinationStatus) => act(async () => {
    const s = await api.settingsGet();
    const next: AppSettings = d.id === "main"
      ? { ...s, stream: { ...s.stream, main_enabled: !d.enabled } }
      : { ...s, stream: { ...s.stream, targets: s.stream.targets.map((t) => (t.id === d.id ? { ...t, enabled: !d.enabled } : t)) } };
    await api.settingsSet(next);
  });
  return (
    <div className="destinations">
      {destinations.map((d) => (
        <label key={d.id} className="destination-row" title={d.reconnecting ? "Reconnecting…" : d.active ? "Live" : d.enabled ? "Enabled" : "Off"}>
          <input type="checkbox" checked={d.enabled} onChange={() => toggle(d)} />
          <span className={`dot ${d.reconnecting ? "warn" : d.active ? "live" : ""}`} />
          <span className="grow">{d.name}</span>
          <span className="muted small">{d.reconnecting ? "reconnecting" : d.active ? "live" : d.enabled ? "ready" : "off"}</span>
        </label>
      ))}
    </div>
  );
}
