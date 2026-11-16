// Typed wrappers around the Rust command surface (src-tauri/src/commands.rs).
import { invoke } from "@tauri-apps/api/core";

export interface ItemInfo {
  id: number;
  name: string;
  type_id: string;
  type_name: string;
  visible: boolean;
  locked: boolean;
  selected: boolean;
  video: boolean;
  audio: boolean;
  /** Folder (group). */
  group: boolean;
  /** Folder containing this item. */
  parent: number | null;
  /** Where the source appears (per source, not per item). */
  show_in: ShowIn;
}

export type ShowIn = "both" | "stream" | "record";

export type TransformAction =
  | "fit" | "stretch" | "center" | "reset"
  | "rotate_cw" | "rotate_ccw" | "rotate_180" | "flip_h" | "flip_v";

/** Edit Transform values (libobs obs_transform_info + crop). */
export interface ItemTransform {
  pos: [number, number];
  rot: number;
  scale: [number, number];
  alignment: number;
  bounds_type: number;
  bounds: [number, number];
  bounds_alignment: number;
  crop: [number, number, number, number];
  source_size: [number, number];
}

export interface OptimizeRecommendation {
  /** 1 = smoothest PC … 5 = best stream. */
  level: number;
  encoder_id: string;
  encoder_name: string;
  hardware: boolean;
  bitrate_kbps: number;
  width: number;
  height: number;
  fps: number;
  encoder_settings: Record<string, unknown>;
  reasons: string[];
  /** Real encode test per resolution/FPS, best first, stopping at the first that passed. */
  trials: { width: number; height: number; fps: number; passed: boolean; skipped_pct: number; lagged_pct: number }[];
}

export interface OptimizeResult {
  measurement: {
    upload_mbps: number | null;
    upload_error: string | null;
    cpu_name: string;
    cpu_threads: number;
    gpu_name: string;
    encoders: EncoderInfo[];
    canvas: [number, number];
    service: string;
  };
  recommendation: OptimizeRecommendation;
}

export interface StreamTarget {
  id: string;
  name: string;
  enabled: boolean;
  service_type: string;
  service_settings: Record<string, unknown>;
  /** Separate video encoder at bitrate_kbps (extra GPU/CPU). */
  own_encoder: boolean;
  bitrate_kbps: number;
}

export interface DestinationStatus {
  id: string;
  name: string;
  enabled: boolean;
  active: boolean;
  reconnecting: boolean;
}

/** A secret's name and whether it is set; values are only read by the Secrets window. */
export interface SecretInfo {
  id: string;
  group: string;
  label: string;
  set: boolean;
}

export interface UpdateInfo {
  current: string;
  available: boolean;
  version: string;
  notes: string;
  /** The release also changes the OBS runtime: only the full package can update it. */
  needs_full_package: boolean;
  full_url: string;
}

export interface AlertStyle {
  enabled: boolean;
  template: string;
  /** File in alert-media ("" = built-in icon). */
  image: string;
  /** File in alert-media, "" = built-in chime, "none" = silent. */
  sound: string;
  volume: number;
  duration_ms: number;
  animation: string;
  text_color: string;
  accent_color: string;
  font_size: number;
  min_amount: number;
  show_message: boolean;
}

export interface AlertSettings {
  styles: Record<string, AlertStyle>;
  gap_ms: number;
}

export interface AlertEvent {
  id: string;
  kind: string;
  platform: string;
  name: string;
  amount: number | null;
  amount_text: string | null;
  count: number | null;
  message: string;
  ts: number;
  test: boolean;
  source?: string;
}

export interface DonateStatus {
  kofi: { enabled: boolean; state: string; public_url: string | null; webhook_url: string | null };
  patreon: { enabled: boolean; state: string };
  streamelements: { enabled: boolean; state: string };
  streamlabs: { enabled: boolean; state: string };
}

export interface AudioInfo {
  name: string;
  type_id: string;
  volume_db: number;
  muted: boolean;
  monitoring: number;
  channel: boolean;
  /** Source captures a Bluetooth headset mic (degrades headset audio while open). */
  bluetooth_mic: boolean;
  advanced: AudioAdvanced;
}

/** OBS "Advanced Audio Properties" of one source. */
export interface AudioAdvanced {
  sync_offset_ms: number;
  /** 0 = left, 0.5 = center, 1 = right. */
  balance: number;
  mono: boolean;
  /** Bit mask of tracks 1-6. */
  tracks: number;
  /** Hidden from the mixer panel. */
  hidden: boolean;
}

export interface MicStatus {
  device: string;
  open: boolean;
  /** "Default" mic is a Bluetooth headset, so it was intentionally not opened. */
  blocked_bluetooth: boolean;
  live_only: boolean;
}

export interface AudioDevice {
  id: string;
  name: string;
  bluetooth: boolean;
  default: boolean;
}

export interface ModuleInfo {
  name: string;
  file: string;
  description: string;
  author: string;
}

export interface PluginsInfo {
  folder: string;
  modules: ModuleInfo[];
  bridges: [string, boolean][];
}

export interface JsPlugin {
  id: string;
  name: string;
  version: string;
  description: string;
  author: string;
  permissions: string[];
  enabled: boolean;
  running: boolean;
  error: string | null;
  manifest_error: string | null;
}

export interface JsPluginLog {
  id: string;
  level: string;
  text: string;
  ts: number;
}

export interface VtuberStatus {
  enabled: boolean;
  users: { app: string; name: string; allowed: boolean }[];
}

export interface CefStatus {
  loaded: boolean;
  bundled: boolean;
  installed: boolean;
  busy: boolean;
  restart_needed: boolean;
  state: string;
  obs_runtime: string;
}

export type Platform = "twitch" | "youtube" | "kick";

export interface ChatMessage {
  id: string;
  platform: Platform;
  msg_id: string | null;
  author_id: string | null;
  author: string;
  color: string | null;
  text: string;
  badges: string[];
  kind: "message" | "paid" | "membership" | "system";
  amount: string | null;
  ts: number;
}

export interface ChatStatus {
  state: "off" | "connecting" | "connected" | "error";
  detail: string;
}

export interface ApiInfo {
  enabled: boolean;
  port: number;
  allow_lan: boolean;
  url: string;
}

export interface OutputStatus {
  streaming: boolean;
  recording: boolean;
  recording_paused: boolean;
  replay: boolean;
  virtualcam: boolean;
}

export interface NamedSets {
  current: string;
  list: string[];
}

export interface StudioState {
  scenes: string[];
  current: string | null;
  studio_mode: boolean;
  preview_scene: string | null;
  items: ItemInfo[];
  audio: AudioInfo[];
  mic: MicStatus;
  outputs: OutputStatus;
  canvas: { width: number; height: number };
  /** Main + extra stream destinations, main first. */
  destinations: DestinationStatus[];
}

export interface TypeInfo {
  id: string;
  name: string;
  video: boolean;
  audio: boolean;
}

export interface FilterInfo {
  name: string;
  type_id: string;
  type_name: string;
  enabled: boolean;
}

export interface EncoderInfo {
  id: string;
  name: string;
  codec: string;
  texture: boolean;
}

export interface Stats {
  fps: number;
  cpu: number;
  render_lagged: number;
  render_total: number;
  encode_skipped: number;
  encode_total: number;
  /** Totals over all live destinations. */
  stream: { kbps: number; dropped: number; total: number; congestion: number; destinations: { id: string; kbps: number; reconnecting: boolean }[] } | null;
  outputs: OutputStatus;
}

export interface AppSettings {
  video: {
    base_width: number;
    base_height: number;
    output_width: number;
    output_height: number;
    fps_num: number;
    fps_den: number;
    scale_type: string;
  };
  output: {
    video_encoder: string;
    video_encoder_settings: Record<string, unknown>;
    audio_bitrate: number;
    record_path: string;
    record_format: string;
    replay_seconds: number;
    /** Bit mask of audio tracks recorded (bit 0 = track 1). */
    record_tracks: number;
    /** Track sent to the stream (0 = track 1). */
    stream_track: number;
  };
  stream: {
    service_type: string;
    service_settings: Record<string, unknown>;
    /** The main destination goes live with "Go Live". */
    main_enabled: boolean;
    /** Extra multistream destinations. */
    targets: StreamTarget[];
  };
  collection: string;
  profile: string;
  /** Action id -> accelerator, e.g. "Ctrl+Shift+F1". */
  hotkeys: Record<string, string>;
  optimize: { level: number; machine: string };
  updates: { auto_check: boolean };
  ui: { low_memory: boolean };
  alerts: AlertSettings;
  donate: { kofi_enabled: boolean; kofi_port: number; patreon_enabled: boolean; streamelements_enabled: boolean; streamlabs_enabled: boolean };
  transition: { type_id: string; duration_ms: number };
  audio: {
    desktop_device: string;
    mic_device: string;
    mic_live_only: boolean;
    monitoring_device: string;
    /** Extra global devices; "" = disabled. */
    desktop2_device: string;
    mic2_device: string;
    mic3_device: string;
    mic4_device: string;
  };
  api: { enabled: boolean; port: number; password: string; allow_lan: boolean };
  /** `js` is managed by jsPluginSetEnabled (the backend ignores it in settingsSet). */
  plugins: { load_obs_studio_plugins: boolean; js: Record<string, string[]> };
  chat: { twitch: string; kick: string; youtube: string };
  vtuber: { enabled: boolean; allowed_apps: string[]; cover_image: string };
  platform_apps: {
    twitch_client_id: string;
    google_client_id: string;
    google_client_secret: string;
    kick_client_id: string;
    kick_client_secret: string;
  };
}

export interface AccountInfo {
  platform: Platform;
  login: string;
  display_name: string;
}

export type ChatAction =
  | { kind: "timeout"; user_id: string; seconds: number }
  | { kind: "ban"; user_id: string }
  | { kind: "unban"; user_id: string }
  | { kind: "delete"; message_id: string }
  | { kind: "mod"; user_id: string }
  | { kind: "vip"; user_id: string };

export interface PopoutState {
  opacity: number;
  click_through: boolean;
  hotkey: string;
}

export type DisplayId = "main" | "program";

export type PropTarget =
  | { kind: "source"; name: string }
  | { kind: "filter"; source: string; filter: string }
  | { kind: "transition" }
  | { kind: "service" }
  | { kind: "target_service"; id: string }
  | { kind: "video_encoder" };

export interface ListItem {
  name: string;
  value: string | number | boolean;
  disabled: boolean;
}

export interface Property {
  name: string;
  description: string;
  long_description: string;
  visible: boolean;
  enabled: boolean;
  type:
    | "bool" | "int" | "float" | "text" | "path" | "list" | "color" | "color_alpha"
    | "button" | "font" | "editable_list" | "frame_rate" | "group" | "unknown";
  min?: number;
  max?: number;
  step?: number;
  suffix?: string;
  slider?: boolean;
  text_type?: "default" | "password" | "multiline" | "info";
  info_type?: "normal" | "warning" | "error";
  path_type?: "file" | "file_save" | "directory";
  filter?: string;
  default_path?: string;
  format?: "int" | "float" | "string" | "bool";
  editable?: boolean;
  items?: ListItem[];
  list_type?: "strings" | "files" | "files_and_urls";
  checkable?: boolean;
  children?: Property[];
}

export interface PropertiesView {
  properties: Property[];
  settings: Record<string, unknown>;
}

export const api = {
  state: () => invoke<StudioState>("get_state"),

  sceneCreate: (name: string) => invoke<string>("scene_create", { name }),
  sceneRemove: (name: string) => invoke<void>("scene_remove", { name }),
  sceneRename: (name: string, newName: string) => invoke<void>("scene_rename", { name, newName }),
  sceneMove: (name: string, delta: number) => invoke<void>("scene_move", { name, delta }),
  sceneSetCurrent: (name: string) => invoke<void>("scene_set_current", { name }),

  sourceTypes: () => invoke<TypeInfo[]>("source_types"),
  existingSources: () => invoke<string[]>("existing_sources"),
  sourceCreate: (typeId: string, name: string) => invoke<string>("source_create", { typeId, name }),
  sourceAddExisting: (name: string) => invoke<void>("source_add_existing", { name }),
  sourceRename: (name: string, newName: string) => invoke<void>("source_rename", { name, newName }),

  itemRemove: (id: number) => invoke<void>("item_remove", { id }),
  itemRemoveSelected: () => invoke<void>("item_remove_selected"),
  itemSetVisible: (id: number, visible: boolean) => invoke<void>("item_set_visible", { id, visible }),
  itemSetLocked: (id: number, locked: boolean) => invoke<void>("item_set_locked", { id, locked }),
  itemSelect: (id: number | null) => invoke<void>("item_select", { id }),
  itemOrder: (id: number, movement: "up" | "down" | "top" | "bottom") =>
    invoke<void>("item_order", { id, movement }),
  itemTransform: (id: number, action: TransformAction) =>
    invoke<void>("item_transform", { id, action }),

  filterTypes: (source: string) => invoke<TypeInfo[]>("filter_types", { source }),
  filters: (source: string) => invoke<FilterInfo[]>("filters", { source }),
  filterAdd: (source: string, typeId: string, name: string) =>
    invoke<string>("filter_add", { source, typeId, name }),
  filterRemove: (source: string, filter: string) => invoke<void>("filter_remove", { source, filter }),
  sourceSetShowIn: (source: string, mode: ShowIn) => invoke<string | null>("source_set_show_in", { source, mode }),
  filterSetEnabled: (source: string, filter: string, enabled: boolean) =>
    invoke<void>("filter_set_enabled", { source, filter, enabled }),
  filterMove: (source: string, filter: string, up: boolean) => invoke<void>("filter_move", { source, filter, up }),

  audioSetVolume: (name: string, db: number) => invoke<void>("audio_set_volume", { name, db }),
  audioSetMuted: (name: string, muted: boolean) => invoke<void>("audio_set_muted", { name, muted }),
  audioSetMonitoring: (name: string, mode: number) => invoke<void>("audio_set_monitoring", { name, mode }),

  properties: (target: PropTarget) => invoke<PropertiesView>("properties_get", { target }),
  propertiesUpdate: (target: PropTarget, patch: Record<string, unknown>) =>
    invoke<PropertiesView>("properties_update", { target, patch }),
  propertiesButton: (target: PropTarget, button: string) =>
    invoke<PropertiesView>("properties_button", { target, button }),

  streamStart: () => invoke<void>("stream_start"),
  recordStart: () => invoke<void>("record_start"),
  replayStart: () => invoke<void>("replay_start"),
  replaySave: () => invoke<void>("replay_save"),
  outputStop: (kind: "stream" | "record" | "replay" | "virtualcam") => invoke<void>("output_stop", { kind }),
  recordPause: (paused: boolean) => invoke<void>("record_pause", { paused }),

  settingsGet: () => invoke<AppSettings>("settings_get"),
  settingsSet: (settings: AppSettings) => invoke<void>("settings_set", { settings }),
  videoEncoders: () => invoke<EncoderInfo[]>("video_encoders"),
  transitionTypes: () => invoke<TypeInfo[]>("transition_types"),

  previewSetBounds: (display: DisplayId, x: number, y: number, width: number, height: number) =>
    invoke<void>("preview_set_bounds", { display, x, y, width, height }),
  previewSetVisible: (display: DisplayId, visible: boolean) => invoke<void>("preview_set_visible", { display, visible }),
  studioModeSet: (enabled: boolean) => invoke<void>("studio_mode_set", { enabled }),
  studioTransition: () => invoke<void>("studio_transition"),

  audioDevices: () => invoke<{ inputs: AudioDevice[]; outputs: AudioDevice[] }>("audio_devices"),
  voiceSetBypass: (source: string, bypass: boolean) => invoke<void>("voice_set_bypass", { source, bypass }),
  voiceBypassed: (source: string) => invoke<boolean>("voice_bypassed", { source }),
  pluginsInfo: () => invoke<PluginsInfo>("plugins_info"),
  pluginsOpenFolder: () => invoke<void>("plugins_open_folder"),
  jsPluginsList: () => invoke<{ folder: string; plugins: JsPlugin[] }>("js_plugins_list"),
  jsPluginSetEnabled: (id: string, enabled: boolean) => invoke<void>("js_plugin_set_enabled", { id, enabled }),
  jsPluginReload: (id: string) => invoke<void>("js_plugin_reload", { id }),
  jsPluginShow: (id: string, visible: boolean) => invoke<void>("js_plugin_show", { id, visible }),
  jsPluginLogs: (id: string) => invoke<JsPluginLog[]>("js_plugin_logs", { id }),
  jsPluginsOpenFolder: () => invoke<void>("js_plugins_open_folder"),
  vtuberStatus: () => invoke<VtuberStatus>("vtuber_status"),
  cefStatus: () => invoke<CefStatus>("cef_status"),
  cefInstall: (path: string | null) => invoke<void>("cef_install", { path }),
  vtuberImportCover: (path: string) => invoke<string>("vtuber_import_cover", { path }),
  apiInfo: () => invoke<ApiInfo>("api_info"),
  chatHistory: () => invoke<ChatMessage[]>("chat_history"),
  chatStatus: () => invoke<Partial<Record<Platform, ChatStatus>>>("chat_status"),
  chatSend: (text: string, platform: Platform | null) => invoke<void>("chat_send", { text, platform }),
  chatAction: (platform: Platform, action: ChatAction) => invoke<void>("chat_action", { platform, action }),
  authAccounts: () => invoke<AccountInfo[]>("auth_accounts"),
  authLogin: (platform: Platform) => invoke<void>("auth_login", { platform }),
  authLogout: (platform: Platform) => invoke<void>("auth_logout", { platform }),
  popoutOpen: () => invoke<void>("popout_open"),
  popoutState: () => invoke<PopoutState>("popout_state"),
  popoutSetOpacity: (opacity: number) => invoke<void>("popout_set_opacity", { opacity }),
  popoutSetClickThrough: (enabled: boolean) => invoke<void>("popout_set_click_through", { enabled }),
  virtualcamToggle: () => invoke<void>("virtualcam_toggle"),
  hotkeyActions: () => invoke<string[]>("hotkey_actions"),
  collections: () => invoke<NamedSets>("collections"),
  collectionSwitch: (name: string) => invoke<void>("collection_switch", { name }),
  collectionCreate: (name: string) => invoke<void>("collection_create", { name }),
  collectionDuplicate: (name: string) => invoke<void>("collection_duplicate", { name }),
  collectionRename: (name: string, newName: string) => invoke<void>("collection_rename", { name, newName }),
  collectionRemove: (name: string) => invoke<void>("collection_remove", { name }),
  profiles: () => invoke<NamedSets>("profiles"),
  profileSwitch: (name: string) => invoke<void>("profile_switch", { name }),
  profileCreate: (name: string) => invoke<void>("profile_create", { name }),
  profileRename: (name: string, newName: string) => invoke<void>("profile_rename", { name, newName }),
  profileRemove: (name: string) => invoke<void>("profile_remove", { name }),
  itemMove: (id: number, parent: number | null, index: number) => invoke<void>("item_move", { id, parent, index }),
  groupCreate: (name: string, ids: number[]) => invoke<number>("group_create", { name, ids }),
  groupUngroup: (id: number) => invoke<void>("group_ungroup", { id }),
  itemPaste: (fromScene: string, fromId: number, duplicate: boolean) =>
    invoke<number>("item_paste", { fromScene, fromId, duplicate }),
  filtersCopy: (from: string, to: string) => invoke<void>("filters_copy", { from, to }),
  itemTransformGet: (id: number) => invoke<ItemTransform>("item_transform_get", { id }),
  itemTransformSet: (id: number, transform: ItemTransform) => invoke<void>("item_transform_set", { id, transform }),
  itemTransitions: (id: number) =>
    invoke<{ show: { type_id: string; duration_ms: number }; hide: { type_id: string; duration_ms: number } }>("item_transitions", { id }),
  itemTransitionSet: (id: number, show: boolean, typeId: string, durationMs: number) =>
    invoke<void>("item_transition_set", { id, show, typeId, durationMs }),
  projectorMonitors: () => invoke<string[]>("projector_monitors"),
  projectorOpen: (monitor: number | null) => invoke<void>("projector_open", { monitor }),
  audioSetAdvanced: (name: string, patch: Partial<AudioAdvanced>) => invoke<void>("audio_set_advanced", { name, patch }),
  optimizeRun: (level: number) => invoke<OptimizeResult>("optimize_run", { level }),
  optimizeMachineCheck: () => invoke<{ new_machine: boolean; machine: string; level: number }>("optimize_machine_check"),
  optimizeDismiss: (level: number) => invoke<void>("optimize_dismiss", { level }),
  secretsOpen: (focus?: string) => invoke<void>("secrets_open", { focus: focus ?? null }),
  secretsList: () => invoke<{ items: SecretInfo[]; lost: boolean }>("secrets_list"),
  secretGet: (id: string) => invoke<string>("secret_get", { id }),
  secretSet: (id: string, value: string) => invoke<void>("secret_set", { id, value }),
  updateCheck: () => invoke<UpdateInfo>("update_check"),
  updateInstall: () => invoke<string>("update_install"),
  updateRestart: () => invoke<void>("update_restart"),
  alertsKinds: () => invoke<[string, string][]>("alerts_kinds"),
  alertsTest: (kind: string) => invoke<void>("alerts_test", { kind }),
  alertsRecent: () => invoke<AlertEvent[]>("alerts_recent"),
  alertsReplay: (id: string) => invoke<void>("alerts_replay", { id }),
  alertsSkip: () => invoke<void>("alerts_skip"),
  alertsImportMedia: (path: string) => invoke<string>("alerts_import_media", { path }),
  alertsAddToScene: () => invoke<number>("alerts_add_to_scene"),
  donateStatus: () => invoke<DonateStatus>("donate_status"),
  optimizeApply: (rec: OptimizeRecommendation) => invoke<void>("optimize_apply", { rec }),
};

export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e);
}
