import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, AppSettings, AudioDevice, EncoderInfo, PluginsInfo, TypeInfo } from "../api";
import { accountsStore, act, authPendingStore, studioStore, toast, useStore } from "../store";
import { Modal } from "./Modal";
import { PropertiesForm } from "./PropertiesForm";
import { overlayUrl, PLATFORMS } from "./ChatView";
import { NamedSetOps, NamedSetPicker } from "./NamedSets";
import { AutoOptimize } from "./AutoOptimize";
import { DestinationsEditor } from "./Destinations";
import { SecretFields, useSecrets } from "./SecretField";

export type SettingsTab = "optimize" | "stream" | "output" | "video" | "audio" | "transitions" | "hotkeys" | "chat" | "accounts" | "plugins" | "api";
type Tab = SettingsTab;
const TABS: { id: Tab; label: string }[] = [
  { id: "optimize", label: "Auto-Optimize" },
  { id: "stream", label: "Stream" },
  { id: "output", label: "Output" },
  { id: "video", label: "Video" },
  { id: "audio", label: "Audio" },
  { id: "transitions", label: "Transitions" },
  { id: "hotkeys", label: "Hotkeys" },
  { id: "chat", label: "Chat" },
  { id: "accounts", label: "Accounts" },
  { id: "plugins", label: "Plugins" },
  { id: "api", label: "Remote API" },
];

function DeviceSelect(props: {
  value: string;
  devices: AudioDevice[];
  onChange: (v: string) => void;
  allowDisable?: boolean;
  defaultLabel?: string;
  /** Only explicit devices (extra global devices never follow the system default). */
  noDefault?: boolean;
}) {
  const known = props.value === "" || props.value === "default" || props.devices.some((d) => d.id === props.value);
  return (
    <select value={props.value} onChange={(e) => props.onChange(e.target.value)}>
      {props.allowDisable && <option value="">Disabled</option>}
      {!props.noDefault && <option value="default">{props.defaultLabel ?? "Default"}</option>}
      {props.devices.map((d) => (
        <option key={d.id} value={d.id}>{d.name}{d.bluetooth ? " — Bluetooth" : ""}{d.default ? " (system default)" : ""}</option>
      ))}
      {!known && <option value={props.value}>Unavailable device</option>}
    </select>
  );
}

const PROFILE_OPS: NamedSetOps = {
  list: api.profiles,
  switchTo: api.profileSwitch,
  create: api.profileCreate,
  rename: api.profileRename,
  remove: api.profileRemove,
};

const HOTKEY_LABELS: Record<string, string> = {
  "stream.toggle": "Start / stop streaming",
  "record.toggle": "Start / stop recording",
  "record.pause": "Pause / resume recording",
  "replay.toggle": "Start / stop replay buffer",
  "replay.save": "Save replay",
  "virtualcam.toggle": "Start / stop virtual camera",
  "studio.transition": "Studio Mode: transition",
  "mute.desktop": "Mute / unmute Desktop Audio",
  "mute.mic": "Mute / unmute Mic/Aux",
};

const MODIFIER_CODES = /^(Control|Shift|Alt|Meta|OS)(Left|Right)?$/;

/** Records a key combination as an accelerator string ("Ctrl+Shift+KeyA"). */
function HotkeyInput(props: { value: string; onChange: (v: string) => void }) {
  const shown = props.value.replace(/Key(?=[A-Z]$)/, "").replace(/Digit(?=\d$)/, "");
  return (
    <div className="prop-path">
      <input readOnly value={shown} placeholder="Click, then press keys"
        onKeyDown={(e) => {
          e.preventDefault();
          if (MODIFIER_CODES.test(e.code)) return;
          const plain = !e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey;
          if (plain && (e.code === "Backspace" || e.code === "Delete" || e.code === "Escape")) return props.onChange("");
          // Some injected input (macro tools, remote desktop) has no physical key code.
          const code = e.code || (/^[a-z]$/i.test(e.key) ? `Key${e.key.toUpperCase()}`
            : /^\d$/.test(e.key) ? `Digit${e.key}` : /^F\d{1,2}$/.test(e.key) ? e.key : "");
          if (!code) return;
          const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super"].filter(Boolean);
          props.onChange([...mods, code].join("+"));
        }} />
      <button disabled={!props.value} onClick={() => props.onChange("")}>Clear</button>
    </div>
  );
}

const RESOLUTIONS = ["3840x2160", "2560x1440", "1920x1080", "1600x900", "1280x720", "1080x1920", "854x480"];
const FPS = [
  { label: "24", num: 24, den: 1 },
  { label: "29.97", num: 30000, den: 1001 },
  { label: "30", num: 30, den: 1 },
  { label: "48", num: 48, den: 1 },
  { label: "59.94", num: 60000, den: 1001 },
  { label: "60", num: 60, den: 1 },
];

function Resolution(props: { w: number; h: number; onChange: (w: number, h: number) => void; disabled?: boolean }) {
  const [text, setText] = useState(`${props.w}x${props.h}`);
  useEffect(() => setText(`${props.w}x${props.h}`), [props.w, props.h]);
  const commit = () => {
    const m = text.trim().match(/^(\d+)\s*[x×]\s*(\d+)$/);
    if (m) props.onChange(+m[1], +m[2]);
    else setText(`${props.w}x${props.h}`);
  };
  return (
    <>
      <input list="resolutions" value={text} disabled={props.disabled} onChange={(e) => setText(e.target.value)}
        onBlur={commit} onKeyDown={(e) => e.key === "Enter" && commit()} />
      <datalist id="resolutions">{RESOLUTIONS.map((r) => <option key={r} value={r} />)}</datalist>
    </>
  );
}

export function SettingsDialog(props: { onClose: () => void; live: boolean; initialTab?: Tab }) {
  const [tab, setTab] = useState<Tab>(props.initialTab ?? "stream");
  const [draft, setDraft] = useState<AppSettings | null>(null);
  const [encoders, setEncoders] = useState<EncoderInfo[]>([]);
  const [transitions, setTransitions] = useState<TypeInfo[]>([]);
  const [formKey, setFormKey] = useState(0);
  const [devices, setDevices] = useState<{ inputs: AudioDevice[]; outputs: AudioDevice[] }>({ inputs: [], outputs: [] });
  const [plugins, setPlugins] = useState<PluginsInfo | null>(null);
  const secrets = useSecrets();
  const secretSet = (id: string) => secrets.some((s) => s.id === id && s.set);
  const [hotkeyActions, setHotkeyActions] = useState<string[]>([]);
  const studio = useStore(studioStore);
  const accounts = useStore(accountsStore);
  const pending = useStore(authPendingStore);

  useEffect(() => {
    api.settingsGet().then(setDraft);
    api.videoEncoders().then(setEncoders);
    api.transitionTypes().then(setTransitions);
    api.audioDevices().then(setDevices);
    api.pluginsInfo().then(setPlugins);
    api.hotkeyActions().then(setHotkeyActions);
  }, []);

  if (!draft) return null;

  /** Fields edited through properties forms are owned by the backend; never overwrite them with the draft. */
  const persist = async (next: AppSettings) => {
    const fresh = await api.settingsGet();
    const merged: AppSettings = {
      ...next,
      output: { ...next.output, video_encoder_settings: fresh.output.video_encoder_settings },
      stream: {
        ...next.stream,
        service_settings: next.stream.service_type === fresh.stream.service_type ? fresh.stream.service_settings : {},
        // Destination service settings are edited through their forms (owned by the backend).
        targets: next.stream.targets.map((t) => {
          const f = fresh.stream.targets.find((x) => x.id === t.id);
          return f && f.service_type === t.service_type ? { ...t, service_settings: f.service_settings } : t;
        }),
      },
    };
    const ok = await act(async () => {
      await api.settingsSet(merged);
      return true;
    });
    if (ok) setDraft(merged);
    return !!ok;
  };

  /** Selects that change which form is shown are saved immediately. */
  const saveNow = async (next: AppSettings) => {
    setDraft(next);
    if (await persist(next)) setFormKey((k) => k + 1);
  };

  const save = async () => {
    if (await persist(draft)) {
      toast("Settings saved");
      props.onClose();
    }
  };

  const set = <K extends Exclude<keyof AppSettings, "collection" | "profile" | "hotkeys">>(section: K, patch: Partial<AppSettings[K]>) =>
    setDraft({ ...draft, [section]: { ...draft[section], ...patch } });

  const fpsLabel = FPS.find((f) => f.num === draft.video.fps_num && f.den === draft.video.fps_den)?.label ?? "30";
  const encoderId = draft.output.video_encoder || encoders[0]?.id || "";

  return (
    <Modal
      title="Settings"
      onClose={props.onClose}
      wide
      footer={
        <>
          <button onClick={props.onClose}>Cancel</button>
          <button className="primary" onClick={save}>Save</button>
        </>
      }
    >
      <div className="settings">
        <nav className="settings-tabs">
          {TABS.map((t) => (
            <button key={t.id} className={tab === t.id ? "active" : ""} onClick={() => setTab(t.id)}>{t.label}</button>
          ))}
        </nav>
        <div className="settings-body">
          {props.live && <div className="prop-info warning">Some settings are locked while streaming or recording.</div>}
          {(tab === "stream" || tab === "output" || tab === "video") && (
            <NamedSetPicker label="Profile" ops={PROFILE_OPS} disabled={props.live}
              onChanged={() => api.settingsGet().then((s) => { setDraft(s); setFormKey((k) => k + 1); })} />
          )}

          {tab === "optimize" && (
            <AutoOptimize live={props.live}
              onApplied={() => api.settingsGet().then((s) => { setDraft(s); setFormKey((k) => k + 1); })} />
          )}

          {tab === "hotkeys" && (() => {
            const setKey = (action: string, accel: string) => {
              const hotkeys = { ...draft.hotkeys };
              if (accel) hotkeys[action] = accel;
              else delete hotkeys[action];
              setDraft({ ...draft, hotkeys });
            };
            const row = (action: string, label: string) => (
              <div className="prop-row" key={action}>
                <span className="prop-label">{label}</span>
                <HotkeyInput value={draft.hotkeys[action] ?? ""} onChange={(v) => setKey(action, v)} />
              </div>
            );
            return (
              <>
                <div className="prop-info">
                  Hotkeys work even while a game is focused. A combination used here is not passed to other apps,
                  so prefer combinations with Ctrl / Alt / Shift. Backspace clears a field.
                </div>
                {hotkeyActions.map((a) => row(a, HOTKEY_LABELS[a] ?? a))}
                <h3>Switch to scene</h3>
                {(studio?.scenes ?? []).map((sc) => row(`scene:${sc}`, sc))}
              </>
            );
          })()}

          {tab === "stream" && (
            <>
              <div className="prop-row">
                <span className="prop-label">Service type</span>
                <select value={draft.stream.service_type} disabled={props.live}
                  onChange={(e) => saveNow({ ...draft, stream: { ...draft.stream, service_type: e.target.value, service_settings: {} } })}>
                  <option value="rtmp_common">Streaming platform (Twitch, YouTube, …)</option>
                  <option value="rtmp_custom">Custom server (RTMP / SRT / RIST)</option>
                  <option value="whip_custom">WHIP / WebRTC (low latency)</option>
                </select>
              </div>
              <PropertiesForm key={`svc-${formKey}`} target={{ kind: "service" }} />
              <SecretFields prefix="main." secrets={secrets} />
              <label className="prop-check">
                <input type="checkbox" checked={draft.stream.main_enabled}
                  onChange={(e) => saveNow({ ...draft, stream: { ...draft.stream, main_enabled: e.target.checked } })} />
                Go live on this main destination
              </label>
              <DestinationsEditor draft={draft} live={props.live} formKey={formKey} saveNow={saveNow} setDraft={setDraft} />
            </>
          )}

          {tab === "output" && (
            <>
              <h3>Encoder</h3>
              <div className="prop-row">
                <span className="prop-label">Video encoder</span>
                <select value={encoderId} disabled={props.live}
                  onChange={(e) => saveNow({ ...draft, output: { ...draft.output, video_encoder: e.target.value } })}>
                  {encoders.map((e) => (
                    <option key={e.id} value={e.id}>{e.name}{e.texture ? " (GPU)" : ""}</option>
                  ))}
                </select>
              </div>
              <PropertiesForm key={`enc-${encoderId}-${formKey}`} target={{ kind: "video_encoder" }} />
              <div className="prop-row">
                <span className="prop-label">Audio bitrate</span>
                <select value={draft.output.audio_bitrate} disabled={props.live}
                  onChange={(e) => set("output", { audio_bitrate: +e.target.value })}>
                  {[96, 128, 160, 192, 256, 320].map((b) => <option key={b} value={b}>{b} kbps</option>)}
                </select>
              </div>

              <h3>Recording</h3>
              <div className="prop-row">
                <span className="prop-label">Folder</span>
                <div className="prop-path">
                  <input value={draft.output.record_path} onChange={(e) => set("output", { record_path: e.target.value })} />
                  <button onClick={async () => {
                    const dir = await open({ directory: true, defaultPath: draft.output.record_path });
                    if (typeof dir === "string") set("output", { record_path: dir });
                  }}>Browse</button>
                </div>
              </div>
              <div className="prop-row">
                <span className="prop-label">Format</span>
                <select value={draft.output.record_format} onChange={(e) => set("output", { record_format: e.target.value })}>
                  <option value="fragmented_mp4">Fragmented MP4 (crash-safe)</option>
                  <option value="mp4">MP4</option>
                  <option value="mkv">MKV</option>
                  <option value="mov">MOV</option>
                  <option value="flv">FLV</option>
                </select>
              </div>
              <div className="prop-row">
                <span className="prop-label">Recording audio tracks</span>
                <div className="row">
                  {[0, 1, 2, 3, 4, 5].map((i) => (
                    <label key={i} className="prop-check">
                      <input type="checkbox" checked={(draft.output.record_tracks & (1 << i)) !== 0}
                        onChange={(e) => {
                          const m = e.target.checked ? draft.output.record_tracks | (1 << i) : draft.output.record_tracks & ~(1 << i);
                          set("output", { record_tracks: m || 1 });
                        }} />
                      {i + 1}
                    </label>
                  ))}
                </div>
              </div>
              <div className="muted small">
                Each track is a separate audio stream in the file (FLV keeps only the first). Choose which sources go to
                which track in Mixer → ⚙ Advanced Audio Properties, e.g. track 2 without music for VODs.
              </div>
              <div className="prop-row">
                <span className="prop-label">Streaming audio track</span>
                <select value={draft.output.stream_track} disabled={props.live}
                  onChange={(e) => set("output", { stream_track: +e.target.value })}>
                  {[0, 1, 2, 3, 4, 5].map((i) => <option key={i} value={i}>Track {i + 1}</option>)}
                </select>
              </div>
              <div className="prop-row">
                <span className="prop-label">Replay buffer length</span>
                <div className="prop-number">
                  <input type="number" min={5} max={21600} value={draft.output.replay_seconds}
                    onChange={(e) => set("output", { replay_seconds: +e.target.value })} />
                  <span className="muted">seconds</span>
                </div>
              </div>
            </>
          )}

          {tab === "video" && (
            <>
              <div className="prop-row">
                <span className="prop-label">Canvas (base) resolution</span>
                <Resolution w={draft.video.base_width} h={draft.video.base_height} disabled={props.live}
                  onChange={(w, h) => set("video", { base_width: w, base_height: h })} />
              </div>
              <div className="prop-row">
                <span className="prop-label">Output (scaled) resolution</span>
                <Resolution w={draft.video.output_width} h={draft.video.output_height} disabled={props.live}
                  onChange={(w, h) => set("video", { output_width: w, output_height: h })} />
              </div>
              <div className="prop-row">
                <span className="prop-label">Downscale filter</span>
                <select value={draft.video.scale_type} disabled={props.live} onChange={(e) => set("video", { scale_type: e.target.value })}>
                  <option value="bilinear">Bilinear (fastest)</option>
                  <option value="area">Area</option>
                  <option value="bicubic">Bicubic</option>
                  <option value="lanczos">Lanczos (sharpest)</option>
                </select>
              </div>
              <div className="prop-row">
                <span className="prop-label">Frame rate</span>
                <select value={fpsLabel} disabled={props.live}
                  onChange={(e) => {
                    const f = FPS.find((x) => x.label === e.target.value)!;
                    set("video", { fps_num: f.num, fps_den: f.den });
                  }}>
                  {FPS.map((f) => <option key={f.label}>{f.label}</option>)}
                </select>
              </div>
            </>
          )}

          {tab === "audio" && (() => {
            const a = draft.audio;
            const micDev = a.mic_device === "default" ? devices.inputs.find((d) => d.default) : devices.inputs.find((d) => d.id === a.mic_device);
            return (
              <>
                <div className="prop-row">
                  <span className="prop-label">Desktop audio</span>
                  <DeviceSelect value={a.desktop_device} devices={devices.outputs} allowDisable
                    onChange={(v) => set("audio", { desktop_device: v })} />
                </div>
                <div className="prop-row">
                  <span className="prop-label">Microphone</span>
                  <DeviceSelect value={a.mic_device} devices={devices.inputs} allowDisable
                    defaultLabel="Default (skips Bluetooth headsets)" onChange={(v) => set("audio", { mic_device: v })} />
                </div>
                {micDev?.bluetooth && a.mic_device === "default" && (
                  <div className="prop-info warning">
                    Your default mic is a Bluetooth headset, so it will not be opened. Opening it switches the headset to
                    hands-free mode (mono 16 kHz) for everything you hear and for captured desktop audio.
                  </div>
                )}
                {micDev?.bluetooth && a.mic_device !== "default" && (
                  <div className="prop-info warning">
                    You selected a Bluetooth headset mic. While it is open your headset audio and desktop audio drop to
                    hands-free quality. Consider "Only while live/recording" below, or a USB / built-in mic.
                  </div>
                )}
                <label className="prop-check">
                  <input type="checkbox" checked={a.mic_live_only} onChange={(e) => set("audio", { mic_live_only: e.target.checked })} />
                  Open the microphone only while streaming / recording / replay buffer is running
                </label>
                <div className="prop-row">
                  <span className="prop-label">Monitoring / preview device</span>
                  <DeviceSelect value={a.monitoring_device} devices={devices.outputs}
                    onChange={(v) => set("audio", { monitoring_device: v })} />
                </div>
                <div className="muted small">
                  Monitoring (and Voice Studio preview) plays here only — it is never added to the stream.
                  Device filters, volume and noise suppression live in the mixer (🎙 / ✨).
                </div>

                <h3>Extra devices</h3>
                <div className="muted small">
                  Appear in the mixer in every scene, like Desktop Audio and Mic/Aux. Extra mics follow the
                  "only while live" option above.
                </div>
                {([["desktop2_device", "Desktop Audio 2", devices.outputs], ["mic2_device", "Mic/Aux 2", devices.inputs],
                  ["mic3_device", "Mic/Aux 3", devices.inputs], ["mic4_device", "Mic/Aux 4", devices.inputs]] as const).map(([key, label, list]) => (
                  <div className="prop-row" key={key}>
                    <span className="prop-label">{label}</span>
                    <DeviceSelect value={a[key]} devices={list} allowDisable noDefault onChange={(v) => set("audio", { [key]: v })} />
                  </div>
                ))}
              </>
            );
          })()}

          {tab === "transitions" && (
            <>
              <div className="prop-row">
                <span className="prop-label">Scene transition</span>
                <select value={draft.transition.type_id}
                  onChange={(e) => saveNow({ ...draft, transition: { ...draft.transition, type_id: e.target.value } })}>
                  {transitions.map((t) => <option key={t.id} value={t.id}>{t.name}</option>)}
                </select>
              </div>
              <div className="prop-row">
                <span className="prop-label">Duration</span>
                <div className="prop-number">
                  <input type="number" min={50} max={20000} step={50} value={draft.transition.duration_ms}
                    onChange={(e) => set("transition", { duration_ms: +e.target.value })} />
                  <span className="muted">ms</span>
                </div>
              </div>
              <PropertiesForm key={`tr-${draft.transition.type_id}-${formKey}`} target={{ kind: "transition" }} />
            </>
          )}

          {tab === "chat" && (
            <>
              <p className="muted">
                Read chat from several platforms at once — no login needed. Leave a field empty to disable it.
                Sending messages and alerts (follows, subs) arrive with platform login in a later update.
              </p>
              <div className="prop-row">
                <span className="prop-label">Twitch channel</span>
                <input placeholder="channel name or twitch.tv URL" value={draft.chat.twitch}
                  onChange={(e) => set("chat", { twitch: e.target.value })} />
              </div>
              <div className="prop-row">
                <span className="prop-label">YouTube</span>
                <input placeholder="@handle, channel URL, or live video URL" value={draft.chat.youtube}
                  onChange={(e) => set("chat", { youtube: e.target.value })} />
              </div>
              <div className="prop-row">
                <span className="prop-label">Kick channel</span>
                <input placeholder="channel name, kick.com URL, or chatroom id" value={draft.chat.kick}
                  onChange={(e) => set("chat", { kick: e.target.value })} />
              </div>
              <div className="muted small">
                YouTube chat is read through the same endpoint as the chat pop-out (unofficial; it may break if YouTube
                changes it). A YouTube channel is only found while it is live.
              </div>
              <h3>Chat overlay</h3>
              <div className="prop-row">
                <span className="prop-label">Browser Source URL</span>
                <div className="prop-path">
                  <input readOnly value={overlayUrl(draft.api.port)} />
                  <button onClick={() => navigator.clipboard.writeText(overlayUrl(draft.api.port)).then(() => toast("URL copied"))}>Copy</button>
                </div>
              </div>
              <div className="muted small">
                Options: <code>fade=30</code> hides messages after 30 s, <code>size=20</code> font size,
                <code>max=20</code> messages on screen, <code>bg=%23000000aa</code> bubble color. Requires the Remote API to be enabled.
              </div>
            </>
          )}

          {tab === "accounts" && (() => {
            const apps = draft.platform_apps;
            const configured: Record<string, boolean> = {
              twitch: !!apps.twitch_client_id,
              youtube: !!apps.google_client_id,
              kick: !!apps.kick_client_id && secretSet("platform.kick_client_secret"),
            };
            return (
              <>
                <p className="muted">
                  Sign in to send chat messages and use moderation (timeout, ban, mod…). Reading chat works without signing in.
                </p>
                {PLATFORMS.map((pl) => {
                  const acc = accounts.find((a) => a.platform === pl.id);
                  const wait = pending[pl.id];
                  return (
                    <div key={pl.id} className="account-row">
                      <span className={`dot ${pl.id}`} />
                      <b className="grow">{pl.label}</b>
                      {acc ? (
                        <>
                          <span className="muted">Signed in as {acc.display_name}</span>
                          <button onClick={() => act(() => api.authLogout(pl.id))}>Sign out</button>
                        </>
                      ) : wait ? (
                        <span className="muted small">
                          {wait.user_code ? <>Enter code <code>{wait.user_code}</code> in the browser window…</> : "Finish signing in in your browser…"}
                        </span>
                      ) : (
                        <button className="primary" disabled={!configured[pl.id]}
                          title={configured[pl.id] ? undefined : "Add the developer app credentials below first"}
                          onClick={async () => { if (await persist(draft)) act(() => api.authLogin(pl.id)); }}>
                          Sign in
                        </button>
                      )}
                    </div>
                  );
                })}
                <div className="muted small">TikTok and Facebook sign-in will come in a later update.</div>
                <h3>Developer apps</h3>
                <p className="muted small">
                  Client IDs from the developer consoles (see docs/platform-apps.md). Release builds include them;
                  fill these in to test a build without them.
                </p>
                {([
                  ["twitch_client_id", "Twitch client ID"],
                  ["google_client_id", "Google client ID"],
                  ["kick_client_id", "Kick client ID"],
                ] as const).map(([key, label]) => (
                  <div key={key} className="prop-row">
                    <span className="prop-label">{label}</span>
                    <input value={apps[key]} onChange={(e) => set("platform_apps", { [key]: e.target.value } as Partial<typeof apps>)} />
                  </div>
                ))}
                <SecretFields prefix="platform." secrets={secrets} />
              </>
            );
          })()}

          {tab === "plugins" && (
            <>
              <p className="muted">
                Streamtubah loads standard OBS Studio plug-ins (sources, filters, encoders, outputs). Put a plug-in in the
                folder below using the OBS layout <code>&lt;name&gt;/bin/64bit/&lt;name&gt;.dll</code> + <code>&lt;name&gt;/data/</code>,
                then restart. VST 2.x voice plug-ins are used through the "VST 2.x Plug-in" filter.
              </p>
              <div className="prop-row">
                <span className="prop-label">Plug-in folder</span>
                <div className="prop-path">
                  <input readOnly value={plugins?.folder ?? ""} />
                  <button onClick={() => act(() => api.pluginsOpenFolder())}>Open</button>
                </div>
              </div>
              <label className="prop-check">
                <input type="checkbox" checked={draft.plugins.load_obs_studio_plugins}
                  onChange={(e) => set("plugins", { load_obs_studio_plugins: e.target.checked })} />
                Also load plug-ins installed for OBS Studio (%ProgramData%\obs-studio\plugins) — restart required
              </label>
              <h3>Loaded modules ({plugins?.modules.length ?? 0})</h3>
              <div className="list module-list">
                {plugins?.modules.map((m) => (
                  <div key={m.file} className="list-row">
                    <span className="grow">{m.name || m.file}</span>
                    <span className="muted small">{m.file}</span>
                  </div>
                ))}
              </div>
              {plugins && plugins.bridges.length > 0 && (
                <>
                  <h3>Audio Bridge channels</h3>
                  {plugins.bridges.map(([name, connected]) => (
                    <div key={name} className="row small">
                      <span className="grow">{name}</span>
                      <span className={connected ? "" : "muted"}>{connected ? "● connected" : "○ waiting"}</span>
                    </div>
                  ))}
                </>
              )}
            </>
          )}

          {tab === "api" && (
            <>
              <p className="muted">
                Local WebSocket API compatible with obs-websocket v5. Stream Deck, Touch Portal, Streamer.bot, SAMMI,
                Mix It Up and other obs-websocket tools can connect with the address and password below. The same port
                serves Audio Bridge connections (<code>/audio/&lt;channel&gt;</code>).
              </p>
              <label className="prop-check">
                <input type="checkbox" checked={draft.api.enabled} onChange={(e) => set("api", { enabled: e.target.checked })} />
                Enable remote API
              </label>
              <div className="prop-row">
                <span className="prop-label">Port</span>
                <input type="number" min={1024} max={65535} value={draft.api.port}
                  onChange={(e) => set("api", { port: +e.target.value })} />
              </div>
              <SecretFields prefix="api." secrets={secrets} />
              <label className="prop-check">
                <input type="checkbox" checked={draft.api.allow_lan} onChange={(e) => set("api", { allow_lan: e.target.checked })} />
                Allow other devices on my network (phones, tablets, second PC)
              </label>
              {draft.api.allow_lan && !secretSet("api.password") && (
                <div className="prop-info error">Set a password before exposing the API to your network.</div>
              )}
              <div className="muted small">Address: ws://{draft.api.allow_lan ? "<this PC's IP>" : "127.0.0.1"}:{draft.api.port}</div>
            </>
          )}
        </div>
      </div>
    </Modal>
  );
}
