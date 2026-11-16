// Small app-wide stores: studio state, live levels/stats, toasts, modal gate.
import { useEffect, useSyncExternalStore } from "react";
import { listen } from "@tauri-apps/api/event";
import { AccountInfo, api, ChatMessage, ChatStatus, errorText, Platform, PopoutState, Stats, StudioState } from "./api";

export function createStore<T>(initial: T) {
  let value = initial;
  const subs = new Set<() => void>();
  return {
    get: () => value,
    set: (v: T) => {
      value = v;
      subs.forEach((s) => s());
    },
    subscribe: (cb: () => void) => {
      subs.add(cb);
      return () => {
        subs.delete(cb);
      };
    },
  };
}

export function useStore<T>(store: ReturnType<typeof createStore<T>>): T {
  return useSyncExternalStore(store.subscribe, store.get);
}

// --- Studio state ------------------------------------------------------------

export const studioStore = createStore<StudioState | null>(null);

let refreshQueued = false;
export function refresh() {
  if (refreshQueued) return;
  refreshQueued = true;
  queueMicrotask(async () => {
    refreshQueued = false;
    try {
      studioStore.set(await api.state());
    } catch (e) {
      toast(errorText(e), "error");
    }
  });
}

/** Runs a backend action, reports errors, then refreshes state. */
export async function act<T>(fn: () => Promise<T>): Promise<T | undefined> {
  try {
    return await fn();
  } catch (e) {
    const text = errorText(e);
    // Something needed a Browser Source without the browser component: offer the download.
    if (text.startsWith("BROWSER_COMPONENT_MISSING")) cefDialogStore.set(true);
    else toast(text, "error");
    return undefined;
  } finally {
    refresh();
  }
}

/** Open state of the browser-component download dialog. */
export const cefDialogStore = createStore(false);

// --- Live data -----------------------------------------------------------------

export const levelsStore = createStore<Record<string, [number, number]>>({});
export const statsStore = createStore<Stats | null>(null);

// --- Chat ------------------------------------------------------------------------

const CHAT_KEEP = 300;
export const chatStore = createStore<ChatMessage[]>([]);
export const chatStatusStore = createStore<Partial<Record<Platform, ChatStatus>>>({});

function pushChat(m: ChatMessage) {
  const list = chatStore.get();
  if (list.length && list[list.length - 1].id === m.id) return;
  const next = list.length >= CHAT_KEEP ? list.slice(list.length - CHAT_KEEP + 1) : list.slice();
  next.push(m);
  chatStore.set(next);
}

// --- Toasts --------------------------------------------------------------------

export interface Toast {
  id: number;
  text: string;
  kind: "info" | "error";
}
export const toastStore = createStore<Toast[]>([]);
let toastId = 0;
export function toast(text: string, kind: Toast["kind"] = "info") {
  const t = { id: ++toastId, text, kind };
  toastStore.set([...toastStore.get(), t]);
  setTimeout(() => toastStore.set(toastStore.get().filter((x) => x.id !== t.id)), kind === "error" ? 7000 : 3500);
}

// --- Display gate: native displays cannot be covered by HTML, so they are
// hidden while any modal or full-screen drawer is open.

const displayHolds = createStore(0);

/** Hides native displays until the returned release function is called. */
export function holdDisplaysHidden(): () => void {
  displayHolds.set(displayHolds.get() + 1);
  let released = false;
  return () => {
    if (released) return;
    released = true;
    displayHolds.set(displayHolds.get() - 1);
  };
}
export function useModalGate() {
  useEffect(() => holdDisplaysHidden(), []);
}
export function useDisplaysHidden() {
  return useStore(displayHolds) > 0;
}

// --- Accounts & pop-out --------------------------------------------------------

export const accountsStore = createStore<AccountInfo[]>([]);
export function refreshAccounts() {
  api.authAccounts().then(accountsStore.set).catch(() => {});
}
export const authPendingStore = createStore<Partial<Record<Platform, { user_code?: string; verification_uri?: string }>>>({});
export const popoutStore = createStore<PopoutState>({ opacity: 0.9, click_through: false, hotkey: "ctrl+alt+c" });

// --- Backend events ------------------------------------------------------------

const outputNames: Record<string, string> = { stream: "Stream", record: "Recording", replay: "Replay buffer", virtualcam: "Virtual camera" };

export function useBackendEvents() {
  useEffect(() => {
    const unlisten = [
      listen("studio:changed", () => refresh()),
      listen<string>("app:error", (e) => toast(e.payload, "error")),
      listen("preview:delete", () => act(() => api.itemRemoveSelected())),
      listen<Record<string, [number, number]>>("obs:levels", (e) => levelsStore.set(e.payload)),
      listen<Stats>("obs:stats", (e) => {
        statsStore.set(e.payload);
        // The refresh on an output "stop" signal can still see the output as active; the
        // per-second stats carry the settled state, so resync the buttons when they differ.
        const shown = studioStore.get()?.outputs;
        if (shown && JSON.stringify(shown) !== JSON.stringify(e.payload.outputs)) refresh();
      }),
      listen<ChatMessage>("chat:message", (e) => pushChat(e.payload)),
      listen<PopoutState>("popout:state", (e) => popoutStore.set(e.payload)),
      listen<{ platform: Platform; state: string; message?: string; login?: string; user_code?: string; verification_uri?: string }>(
        "auth:status",
        (e) => {
          const { platform, state, message, login, user_code, verification_uri } = e.payload;
          const pending = { ...authPendingStore.get() };
          if (state === "pending") pending[platform] = { user_code, verification_uri };
          else delete pending[platform];
          authPendingStore.set(pending);
          if (state === "done") toast(`Signed in to ${platform} as ${login}`);
          if (state === "error") toast(`${platform} sign-in failed: ${message}`, "error");
          refreshAccounts();
        },
      ),
      listen<{ platform: Platform } & ChatStatus>("chat:status", (e) => {
        const { platform, state, detail } = e.payload;
        chatStatusStore.set({ ...chatStatusStore.get(), [platform]: { state, detail } });
      }),
      listen<{ output: string; label?: string; signal: string; error?: string; path?: string }>("obs:output", (e) => {
        const { output, label, signal, error, path } = e.payload;
        const name = label ?? outputNames[output] ?? output;
        if (error) toast(`${name}: ${error}`, "error");
        else if (signal === "start") toast(`${name} started`);
        else if (signal === "stop") toast(`${name} stopped`);
        else if (signal === "reconnect") toast(`${name}: reconnecting…`, "error");
        else if (signal === "reconnect_success") toast(`${name}: reconnected`);
        else if (signal === "saved") toast(`Replay saved${path ? `: ${path}` : ""}`);
        refresh();
      }),
    ];
    refresh();
    api.chatHistory().then(chatStore.set).catch(() => {});
    api.chatStatus().then(chatStatusStore.set).catch(() => {});
    api.popoutState().then(popoutStore.set).catch(() => {});
    refreshAccounts();
    return () => unlisten.forEach((p) => p.then((u) => u()));
  }, []);
}
