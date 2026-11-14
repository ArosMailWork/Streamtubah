// Bottom dock panels: scenes, sources, mixer.
import { useEffect, useRef, useState } from "react";
import { api, AudioAdvanced, AudioInfo, ItemInfo, ShowIn, StudioState, TransformAction } from "../api";
import { act, levelsStore, toast } from "../store";
import { AddSourceDialog, FiltersDialog, PropertiesDialog } from "./Dialogs";
import { Modal, PromptDialog } from "./Modal";
import { VoiceDialog } from "./VoiceDialog";
import { NamedSetOps, NamedSetPicker } from "./NamedSets";
import { ContextMenu, MenuEntry } from "./ContextMenu";
import { EditTransformDialog, ItemTransitionDialog } from "./TransformDialogs";
import { useDockHandle } from "../layout/dock";

const SHOW_IN: [ShowIn, string][] = [
  ["both", "Stream and recording"],
  ["stream", "Stream only (not recorded)"],
  ["record", "Recording only (not on stream)"],
];

export function Panel(props: { title: string; actions?: React.ReactNode; children: React.ReactNode; className?: string }) {
  // The header is the drag handle for docking; ✕ removes the panel from the layout.
  const { handle, close } = useDockHandle();
  return (
    <section className={`panel ${props.className ?? ""}`}>
      <header {...handle}>
        <h3>{props.title}</h3>
        <div className="panel-actions">
          {props.actions}
          {close && <button className="icon small dock-close" title="Close panel" onClick={close}>✕</button>}
        </div>
      </header>
      <div className="panel-body">{props.children}</div>
    </section>
  );
}

// --- Scenes --------------------------------------------------------------------

const COLLECTION_OPS: NamedSetOps = {
  list: api.collections,
  switchTo: api.collectionSwitch,
  create: api.collectionCreate,
  duplicate: api.collectionDuplicate,
  rename: api.collectionRename,
  remove: api.collectionRemove,
};

export function ScenesPanel({ state }: { state: StudioState }) {
  const [dialog, setDialog] = useState<null | "add" | "rename">(null);
  // In Studio Mode the list selects the preview scene; the on-air scene gets a LIVE badge.
  const selected = state.studio_mode ? state.preview_scene : state.current;
  const current = selected;
  return (
    <Panel
      title="Scenes"
      actions={
        <>
          <button className="icon" title="Add scene" onClick={() => setDialog("add")}>＋</button>
          <button className="icon" title="Remove scene" disabled={!current || state.scenes.length < 2}
            onClick={() => current && act(() => api.sceneRemove(current))}>－</button>
          <button className="icon" title="Move up" disabled={!current} onClick={() => current && act(() => api.sceneMove(current, -1))}>▲</button>
          <button className="icon" title="Move down" disabled={!current} onClick={() => current && act(() => api.sceneMove(current, 1))}>▼</button>
        </>
      }
    >
      <NamedSetPicker label="Collection" ops={COLLECTION_OPS}
        disabled={state.outputs.streaming || state.outputs.recording || state.outputs.replay || state.outputs.virtualcam} />
      <div className="list">
        {state.scenes.map((s) => (
          <div key={s} className={`list-row ${s === selected ? "active" : ""}`}
            onClick={() => act(() => api.sceneSetCurrent(s))} onDoubleClick={() => setDialog("rename")}>
            <span className="grow">{s}</span>
            {state.studio_mode && s === state.current && <span className="badge live" title="On air">LIVE</span>}
          </div>
        ))}
      </div>
      {dialog === "add" && (
        <PromptDialog title="Add Scene" label="Scene name" initial={`Scene ${state.scenes.length + 1}`} confirm="Add"
          onSubmit={(n) => act(() => api.sceneCreate(n))} onClose={() => setDialog(null)} />
      )}
      {dialog === "rename" && current && (
        <PromptDialog title="Rename Scene" label="Scene name" initial={current} confirm="Rename"
          onSubmit={(n) => act(() => api.sceneRename(current, n))} onClose={() => setDialog(null)} />
      )}
    </Panel>
  );
}

// --- Sources -------------------------------------------------------------------

type SourceDialog =
  | { kind: "add" }
  | { kind: "props"; name: string }
  | { kind: "filters"; name: string }
  | { kind: "rename"; name: string }
  | { kind: "folder"; id?: number }
  | { kind: "transform"; id: number; name: string }
  | { kind: "transition"; id: number; name: string };

/** App-wide clipboards (like OBS): a copied scene item and a copied filter set. */
let copiedItem: { scene: string; id: number; name: string } | null = null;
let copiedFilters: string | null = null;

export function SourcesPanel({ state }: { state: StudioState }) {
  const [dialog, setDialog] = useState<SourceDialog | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; item: ItemInfo | null } | null>(null);
  const [collapsed, setCollapsed] = useState<Set<number>>(new Set());
  const [dragId, setDragId] = useState<number | null>(null);
  const [dropHint, setDropHint] = useState<{ id: number; mode: "before" | "into" } | null>(null);
  const selected: ItemInfo | undefined = state.items.find((i) => i.selected);
  const scene = (state.studio_mode ? state.preview_scene : state.current) ?? "";
  const close = () => setDialog(null);

  const rows = state.items.filter((i) => i.parent === null || !collapsed.has(i.parent));
  const siblings = (parent: number | null) => state.items.filter((i) => i.parent === parent);

  const copy = (it: ItemInfo) => { copiedItem = { scene, id: it.id, name: it.name }; };
  const paste = (duplicate: boolean) => {
    if (copiedItem) { const c = copiedItem; act(() => api.itemPaste(c.scene, c.id, duplicate)); }
  };

  /** Drops the dragged item before `target` (in target's container), into a folder, or at the end. */
  const drop = (target: ItemInfo | null, mode: "before" | "into") => {
    const id = dragId;
    setDragId(null);
    setDropHint(null);
    if (id === null || id === target?.id) return;
    if (target && mode === "into") {
      act(() => api.itemMove(id, target.id, 0));
      return;
    }
    const parent = target ? target.parent : null;
    const list = siblings(parent).filter((i) => i.id !== id);
    const index = target ? list.findIndex((i) => i.id === target.id) : list.length;
    act(() => api.itemMove(id, parent, Math.max(0, index)));
  };

  const menuFor = (it: ItemInfo | null): MenuEntry[] => {
    const pasteEntries: MenuEntry[] = [
      { label: copiedItem ? `Paste "${copiedItem.name}" (reference)` : "Paste (reference)", disabled: !copiedItem, onClick: () => paste(false) },
      { label: "Paste (duplicate)", disabled: !copiedItem, onClick: () => paste(true) },
    ];
    if (!it) {
      return [
        { label: "Add Source…", onClick: () => setDialog({ kind: "add" }) },
        { label: "New Folder…", onClick: () => setDialog({ kind: "folder" }) },
        ...pasteEntries,
      ];
    }
    const order = (m: "top" | "up" | "down" | "bottom") => () => act(() => api.itemOrder(it.id, m));
    const transform = (a: TransformAction) => () => act(() => api.itemTransform(it.id, a));
    const entries: MenuEntry[] = [];
    if (!it.group) {
      entries.push(
        { label: "Properties", onClick: () => setDialog({ kind: "props", name: it.name }) },
        { label: "Filters", onClick: () => setDialog({ kind: "filters", name: it.name }) },
      );
    }
    entries.push({ label: "Rename", onClick: () => setDialog({ kind: "rename", name: it.name }) }, "-");
    if (it.group) {
      entries.push({ label: "Ungroup Folder", onClick: () => act(() => api.groupUngroup(it.id)) });
    } else {
      entries.push(
        { label: "Copy", onClick: () => copy(it) },
        { label: "Duplicate", onClick: () => act(() => api.itemPaste(scene, it.id, true)) },
        ...pasteEntries,
        ...(it.video ? [{ label: "Show in", submenu: SHOW_IN.map(([mode, label]) => ({
          label: `${it.show_in === mode ? "✓ " : ""}${label}`,
          onClick: () => act(async () => { const note = await api.sourceSetShowIn(it.name, mode); if (note) toast(note); }),
        })) } as MenuEntry] : []),
        { label: "Copy Filters", onClick: () => { copiedFilters = it.name; } },
        {
          label: copiedFilters ? `Paste Filters from "${copiedFilters}"` : "Paste Filters",
          disabled: !copiedFilters || copiedFilters === it.name,
          onClick: () => { const from = copiedFilters!; act(() => api.filtersCopy(from, it.name)); },
        },
        "-",
        it.parent === null
          ? { label: "Move into New Folder…", onClick: () => setDialog({ kind: "folder", id: it.id }) }
          : { label: "Move out of Folder", onClick: () => act(() => api.itemMove(it.id, null, 0)) },
      );
    }
    entries.push(
      { label: "Order", submenu: [
        { label: "Move to Top", onClick: order("top") },
        { label: "Move Up", onClick: order("up") },
        { label: "Move Down", onClick: order("down") },
        { label: "Move to Bottom", onClick: order("bottom") },
      ] },
      { label: "Transform", submenu: [
        { label: "Edit Transform…", onClick: () => setDialog({ kind: "transform", id: it.id, name: it.name }) },
        "-",
        { label: "Fit to Screen", onClick: transform("fit") },
        { label: "Stretch to Screen", onClick: transform("stretch") },
        { label: "Center", onClick: transform("center") },
        { label: "Reset", onClick: transform("reset") },
        "-",
        { label: "Rotate 90° Clockwise", onClick: transform("rotate_cw") },
        { label: "Rotate 90° Counter-clockwise", onClick: transform("rotate_ccw") },
        { label: "Rotate 180°", onClick: transform("rotate_180") },
        { label: "Flip Horizontal", onClick: transform("flip_h") },
        { label: "Flip Vertical", onClick: transform("flip_v") },
      ] },
      { label: "Show / Hide Transition…", onClick: () => setDialog({ kind: "transition", id: it.id, name: it.name }) },
      "-",
      { label: it.visible ? "Hide" : "Show", onClick: () => act(() => api.itemSetVisible(it.id, !it.visible)) },
      { label: it.locked ? "Unlock" : "Lock", onClick: () => act(() => api.itemSetLocked(it.id, !it.locked)) },
      { label: "Remove", danger: true, onClick: () => act(() => api.itemRemove(it.id)) },
    );
    return entries;
  };

  const toggleCollapsed = (id: number) => {
    const next = new Set(collapsed);
    if (next.has(id)) next.delete(id); else next.add(id);
    setCollapsed(next);
  };

  return (
    <Panel
      title="Sources"
      className="sources"
      actions={
        <>
          <button className="icon" title="Add source" onClick={() => setDialog({ kind: "add" })}>＋</button>
          <button className="icon" title="New folder" onClick={() => setDialog({ kind: "folder" })}>📁</button>
          <button className="icon" title="Remove" disabled={!selected} onClick={() => selected && act(() => api.itemRemove(selected.id))}>－</button>
          <button className="icon" title="Properties" disabled={!selected || selected.group} onClick={() => selected && setDialog({ kind: "props", name: selected.name })}>⚙</button>
          <button className="icon" title="Filters" disabled={!selected || selected.group} onClick={() => selected && setDialog({ kind: "filters", name: selected.name })}>✨</button>
          <button className="icon" title="More (or right-click a source)" disabled={!selected}
            onClick={(e) => selected && setMenu({ x: e.clientX, y: e.clientY, item: selected })}>⋯</button>
        </>
      }
    >
      <div className="list" tabIndex={0}
        onContextMenu={(e) => { e.preventDefault(); setMenu({ x: e.clientX, y: e.clientY, item: null }); }}
        onKeyDown={(e) => {
          if (!(e.ctrlKey || e.metaKey)) return;
          const key = e.key.toLowerCase();
          if (key === "c" && selected && !selected.group) { copy(selected); e.preventDefault(); }
          if (key === "v") { paste(false); e.preventDefault(); }
          if (key === "d" && selected && !selected.group) { act(() => api.itemPaste(scene, selected.id, true)); e.preventDefault(); }
        }}
        onDragOver={(e) => { if (dragId !== null) e.preventDefault(); }}
        onDrop={(e) => { e.preventDefault(); drop(null, "before"); }}>
        {rows.map((it) => {
          const hint = dropHint?.id === it.id ? `drop-${dropHint.mode}` : "";
          return (
            <div key={it.id} draggable
              className={`list-row ${it.selected ? "active" : ""} ${it.parent !== null ? "child" : ""} ${it.group ? "folder" : ""} ${hint} ${dragId === it.id ? "dragging" : ""}`}
              onClick={() => act(() => api.itemSelect(it.id))}
              onDoubleClick={() => setDialog(it.group ? { kind: "rename", name: it.name } : { kind: "props", name: it.name })}
              onContextMenu={(e) => {
                e.preventDefault();
                e.stopPropagation();
                act(() => api.itemSelect(it.id));
                setMenu({ x: e.clientX, y: e.clientY, item: it });
              }}
              onDragStart={(e) => { setDragId(it.id); e.dataTransfer.effectAllowed = "move"; }}
              onDragEnd={() => { setDragId(null); setDropHint(null); }}
              onDragOver={(e) => {
                if (dragId === null) return;
                e.preventDefault();
                e.stopPropagation();
                const dragged = state.items.find((i) => i.id === dragId);
                const r = e.currentTarget.getBoundingClientRect();
                const into = it.group && !dragged?.group && e.clientY > r.top + r.height * 0.3;
                setDropHint({ id: it.id, mode: into ? "into" : "before" });
              }}
              onDrop={(e) => {
                e.preventDefault();
                e.stopPropagation();
                drop(it, dropHint?.id === it.id ? dropHint.mode : "before");
              }}>
              {it.group && (
                <button className="icon small" title={collapsed.has(it.id) ? "Expand" : "Collapse"}
                  onClick={(e) => { e.stopPropagation(); toggleCollapsed(it.id); }}>{collapsed.has(it.id) ? "▸" : "▾"}</button>
              )}
              <span className="grow" title={it.type_name}>{it.group ? "📁 " : ""}{it.name}</span>
              {it.show_in !== "both" && (
                <span className="badge" title={it.show_in === "stream" ? "Stream only: not in recordings / replay buffer" : "Recording only: not on stream, preview or virtual camera"}>
                  {it.show_in === "stream" ? "📡 only" : "⏺ only"}
                </span>
              )}
              <button className={`icon small ${it.locked ? "on" : ""}`} title={it.locked ? "Unlock" : "Lock"}
                onClick={(e) => { e.stopPropagation(); act(() => api.itemSetLocked(it.id, !it.locked)); }}>{it.locked ? "🔒" : "🔓"}</button>
              <button className={`icon small ${it.visible ? "" : "off"}`} title={it.visible ? "Hide" : "Show"}
                onClick={(e) => { e.stopPropagation(); act(() => api.itemSetVisible(it.id, !it.visible)); }}>{it.visible ? "👁" : "◌"}</button>
            </div>
          );
        })}
        {state.items.length === 0 && (
          <button className="empty-cta" onClick={() => setDialog({ kind: "add" })}>＋ Add your first source</button>
        )}
      </div>

      {menu && <ContextMenu x={menu.x} y={menu.y} entries={menuFor(menu.item)} onClose={() => setMenu(null)} />}
      {dialog?.kind === "add" && (
        <AddSourceDialog onClose={close} onCreated={(name) => setDialog({ kind: "props", name })} />
      )}
      {dialog?.kind === "props" && (
        <PropertiesDialog title={`Properties — ${dialog.name}`} target={{ kind: "source", name: dialog.name }} onClose={close} />
      )}
      {dialog?.kind === "filters" && <FiltersDialog source={dialog.name} onClose={close} />}
      {dialog?.kind === "rename" && (
        <PromptDialog title="Rename" label="Name" initial={dialog.name} confirm="Rename"
          onSubmit={(n) => act(() => api.sourceRename(dialog.name, n))} onClose={close} />
      )}
      {dialog?.kind === "transform" && <EditTransformDialog id={dialog.id} name={dialog.name} onClose={close} />}
      {dialog?.kind === "transition" && <ItemTransitionDialog id={dialog.id} name={dialog.name} onClose={close} />}
      {dialog?.kind === "folder" && (
        <PromptDialog title="New Folder" label="Folder name" initial="Folder" confirm="Create"
          onSubmit={(n) => act(() => api.groupCreate(n, dialog.id !== undefined ? [dialog.id] : []))} onClose={close} />
      )}
    </Panel>
  );
}

// --- Mixer ---------------------------------------------------------------------

const MIN_DB = -60;
const CLIP_DB = -0.5;
const HOLD_MS = 1500;
const SCALE_TICKS = [0, -10, -20, -30, -40, -50, -60];
const VERTICAL_KEY = "streamtubah.mixer.vertical";

const dbToPct = (db: number) => Math.max(0, Math.min(1, (db - MIN_DB) / -MIN_DB)) * 100;

/**
 * Level meter updated straight from the store to avoid re-rendering React at 30 Hz.
 * Shows a peak-hold marker and lights the clip indicator when a channel reaches 0 dB.
 */
export function Meter({ name, vertical }: { name: string; vertical?: boolean }) {
  const bars = [useRef<HTMLDivElement>(null), useRef<HTMLDivElement>(null)];
  const holds = [useRef<HTMLDivElement>(null), useRef<HTMLDivElement>(null)];
  const clip = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const hold = [{ db: -100, at: 0 }, { db: -100, at: 0 }];
    let clipAt = 0;
    const prop = vertical ? "height" : "width";
    const pos = vertical ? "bottom" : "left";
    const update = () => {
      const now = performance.now();
      const peaks = levelsStore.get()[name] ?? [-100, -100];
      bars.forEach((b, i) => {
        if (!b.current) return;
        // The track shows the full color scale; this dark cover hides the part above the level.
        b.current.style[prop] = `${100 - dbToPct(peaks[i])}%`;
        if (peaks[i] >= hold[i].db || now - hold[i].at > HOLD_MS) hold[i] = { db: peaks[i], at: now };
        const h = holds[i].current;
        if (h) {
          h.style[pos] = `${dbToPct(hold[i].db)}%`;
          h.style.opacity = hold[i].db > MIN_DB ? "1" : "0";
        }
        if (peaks[i] >= CLIP_DB) clipAt = now;
      });
      clip.current?.classList.toggle("on", now - clipAt < HOLD_MS);
    };
    update();
    return levelsStore.subscribe(update);
  }, [name, vertical]);
  return (
    <div className={`meter ${vertical ? "vertical" : ""}`}>
      <div className="meter-bars">
        {bars.map((b, i) => (
          <div key={i} className="meter-track">
            <div className="meter-fill" ref={b} />
            <div className="meter-hold" ref={holds[i]} />
          </div>
        ))}
      </div>
      <div className="meter-clip" ref={clip} title="Clipping: lower the volume or add a Limiter filter" />
    </div>
  );
}

function MeterScale({ vertical }: { vertical?: boolean }) {
  return (
    <div className={`meter-scale ${vertical ? "vertical" : ""}`}>
      {SCALE_TICKS.map((db) => (
        <span key={db} style={{ [vertical ? "bottom" : "left"]: `${dbToPct(db)}%` }}>{db}</span>
      ))}
    </div>
  );
}

type MixerAction = "props" | "filters" | "voice" | "advanced";

function MixerRow({ a, vertical, onAction }: { a: AudioInfo; vertical: boolean; onAction: (k: MixerAction, e?: React.MouseEvent) => void }) {
  const [db, setDb] = useState(a.volume_db);
  useEffect(() => setDb(a.volume_db), [a.volume_db]);
  const fader = (
    <input type="range" className={vertical ? "fader-vertical" : ""} min={MIN_DB} max={0} step={0.1} value={db}
      title={`${db.toFixed(1)} dB`}
      onChange={(e) => {
        const v = +e.target.value;
        setDb(v);
        api.audioSetVolume(a.name, v).catch(() => {});
      }}
      onDoubleClick={() => { setDb(0); api.audioSetVolume(a.name, 0).catch(() => {}); }} />
  );
  const mute = (
    <button className={`icon small ${a.muted ? "danger-on" : ""}`} title={a.muted ? "Unmute" : "Mute"}
      onClick={() => act(() => api.audioSetMuted(a.name, !a.muted))}>{a.muted ? "🔇" : "🔊"}</button>
  );
  const more = (
    <button className="icon small" title="Filters, voice, properties, advanced" onClick={(e) => onAction("advanced", e)}>⋯</button>
  );

  if (vertical) {
    return (
      <div className={`mixer-col ${a.muted ? "muted-row" : ""}`} onContextMenu={(e) => { e.preventDefault(); onAction("advanced", e); }}>
        <span className="muted small">{db <= MIN_DB ? "-∞" : db.toFixed(1)}</span>
        <div className="mixer-col-body">
          <Meter name={a.name} vertical />
          {fader}
        </div>
        <div className="row">{mute}{more}</div>
        <span className="mixer-col-name" title={a.name}>{a.name}</span>
      </div>
    );
  }
  return (
    <div className={`mixer-row ${a.muted ? "muted-row" : ""}`} onContextMenu={(e) => { e.preventDefault(); onAction("advanced", e); }}>
      <div className="mixer-head">
        <span className="grow">{a.name}</span>
        {a.bluetooth_mic && <span className="badge warn" title="Bluetooth headset mic: lowers headset audio to hands-free quality while open">BT mic</span>}
        {a.advanced.mono && <span className="badge" title="Mixed down to mono">MONO</span>}
        {a.advanced.sync_offset_ms !== 0 && <span className="badge" title="Sync offset">{a.advanced.sync_offset_ms} ms</span>}
        <span className="muted small">{db <= MIN_DB ? "-∞" : db.toFixed(1)} dB</span>
        <select className="small" value={a.monitoring} title="Audio monitoring"
          onChange={(e) => act(() => api.audioSetMonitoring(a.name, +e.target.value))}>
          <option value={0}>Monitor off</option>
          <option value={1}>Monitor only</option>
          <option value={2}>Monitor + output</option>
        </select>
        <button className="icon small" title="Voice Studio (effects, plug-ins, preview)" onClick={() => onAction("voice")}>🎙</button>
        <button className="icon small" title="Filters" onClick={() => onAction("filters")}>✨</button>
        {more}
        {mute}
      </div>
      <Meter name={a.name} />
      {fader}
    </div>
  );
}

/** OBS-style table of every audio source's advanced settings. */
function AdvancedAudioDialog({ audio, onClose }: { audio: AudioInfo[]; onClose: () => void }) {
  const set = (name: string, patch: Partial<AudioAdvanced>) => act(() => api.audioSetAdvanced(name, patch));
  return (
    <Modal title="Advanced Audio Properties" onClose={onClose} wide footer={<button className="primary" onClick={onClose}>Done</button>}>
      <div className="prop-info small">
        Sync offset delays a source to line it up with video (e.g. a webcam that lags). Tracks choose which recording tracks the source is
        heard on; the stream uses track 1.
      </div>
      <table className="adv-audio">
        <thead>
          <tr><th>Source</th><th>Mono</th><th>Balance (L/R)</th><th>Sync offset</th><th>Tracks</th><th>In mixer</th></tr>
        </thead>
        <tbody>
          {audio.map((a) => (
            <tr key={a.name}>
              <td>{a.name}</td>
              <td><input type="checkbox" checked={a.advanced.mono} onChange={(e) => set(a.name, { mono: e.target.checked })} /></td>
              <td>
                <input type="range" min={0} max={1} step={0.01} value={a.advanced.balance}
                  onChange={(e) => set(a.name, { balance: +e.target.value })} onDoubleClick={() => set(a.name, { balance: 0.5 })}
                  title="Double-click to center" />
              </td>
              <td>
                <input type="number" className="num-small" min={-950} max={20000} step={10} defaultValue={a.advanced.sync_offset_ms}
                  onBlur={(e) => set(a.name, { sync_offset_ms: Math.round(+e.target.value) })} /> ms
              </td>
              <td className="tracks">
                {[0, 1, 2, 3, 4, 5].map((i) => (
                  <label key={i} title={`Track ${i + 1}`}>
                    <input type="checkbox" checked={(a.advanced.tracks & (1 << i)) !== 0}
                      onChange={(e) => set(a.name, { tracks: e.target.checked ? a.advanced.tracks | (1 << i) : a.advanced.tracks & ~(1 << i) })} />
                    {i + 1}
                  </label>
                ))}
              </td>
              <td><input type="checkbox" checked={!a.advanced.hidden} onChange={(e) => set(a.name, { hidden: !e.target.checked })} /></td>
            </tr>
          ))}
        </tbody>
      </table>
    </Modal>
  );
}

export function MixerPanel({ state }: { state: StudioState }) {
  const [dialog, setDialog] = useState<{ kind: "props" | "filters" | "voice"; name: string } | { kind: "advanced" } | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; a: AudioInfo } | null>(null);
  const [vertical, setVertical] = useState(() => localStorage.getItem(VERTICAL_KEY) === "1");
  const [showHidden, setShowHidden] = useState(false);
  const mic = state.mic;
  const hiddenCount = state.audio.filter((a) => a.advanced.hidden).length;
  const shown = state.audio.filter((a) => showHidden || !a.advanced.hidden);

  const onAction = (a: AudioInfo) => (k: MixerAction, e?: React.MouseEvent) => {
    if (k === "advanced") setMenu({ x: e?.clientX ?? 0, y: e?.clientY ?? 0, a });
    else setDialog({ kind: k, name: a.name });
  };
  const menuFor = (a: AudioInfo): MenuEntry[] => [
    { label: a.muted ? "Unmute" : "Mute", onClick: () => act(() => api.audioSetMuted(a.name, !a.muted)) },
    { label: "Filters", onClick: () => setDialog({ kind: "filters", name: a.name }) },
    { label: "Voice Studio", onClick: () => setDialog({ kind: "voice", name: a.name }) },
    { label: "Properties", onClick: () => setDialog({ kind: "props", name: a.name }) },
    "-",
    { label: a.advanced.mono ? "Turn off Mono" : "Mix down to Mono", onClick: () => act(() => api.audioSetAdvanced(a.name, { mono: !a.advanced.mono })) },
    { label: "Advanced Audio Properties…", onClick: () => setDialog({ kind: "advanced" }) },
    { label: a.advanced.hidden ? "Show in Mixer" : "Hide from Mixer", onClick: () => act(() => api.audioSetAdvanced(a.name, { hidden: !a.advanced.hidden })) },
  ];

  return (
    <Panel title="Mixer" className="mixer"
      actions={<>
        {hiddenCount > 0 && (
          <button className={`icon small ${showHidden ? "on" : ""}`} title={`${hiddenCount} hidden source(s)`} onClick={() => setShowHidden(!showHidden)}>
            {showHidden ? "Hide hidden" : `+${hiddenCount} hidden`}
          </button>
        )}
        <button className="icon" title="Advanced Audio Properties" onClick={() => setDialog({ kind: "advanced" })}>⚙</button>
        <button className="icon" title={vertical ? "Horizontal mixer" : "Vertical mixer"}
          onClick={() => { localStorage.setItem(VERTICAL_KEY, vertical ? "0" : "1"); setVertical(!vertical); }}>{vertical ? "☰" : "⫴"}</button>
        <button className="icon" title="Voice Studio" onClick={() => setDialog({ kind: "voice", name: "Mic/Aux" })}>🎙 Voice</button>
      </>}>
      {mic.blocked_bluetooth && (
        <div className="prop-info warning small">
          Mic not opened: your default microphone is a Bluetooth headset. Opening it would drop the headset to
          hands-free quality for you and for desktop audio. Pick another mic, or select the headset explicitly in Settings → Audio.
        </div>
      )}
      {!mic.open && mic.live_only && !mic.blocked_bluetooth && mic.device !== "" && (
        <div className="muted small">Mic opens automatically when you go live or record.</div>
      )}
      {!mic.open && !mic.live_only && !mic.blocked_bluetooth && mic.device !== "" && (
        <div className="muted small">No microphone detected — connect one or choose a device in Settings → Audio.</div>
      )}
      {vertical ? (
        <div className="mixer-vertical">
          <MeterScale vertical />
          {shown.map((a) => <MixerRow key={a.name} a={a} vertical onAction={onAction(a)} />)}
        </div>
      ) : (
        <>
          {shown.length > 0 && <MeterScale />}
          {shown.map((a) => <MixerRow key={a.name} a={a} vertical={false} onAction={onAction(a)} />)}
        </>
      )}
      {menu && <ContextMenu x={menu.x} y={menu.y} entries={menuFor(menu.a)} onClose={() => setMenu(null)} />}
      {dialog?.kind === "advanced" && <AdvancedAudioDialog audio={state.audio} onClose={() => setDialog(null)} />}
      {dialog?.kind === "voice" && <VoiceDialog state={state} initial={dialog.name} onClose={() => setDialog(null)} />}
      {dialog?.kind === "props" && (
        <PropertiesDialog title={`Properties — ${dialog.name}`} target={{ kind: "source", name: dialog.name }} onClose={() => setDialog(null)} />
      )}
      {dialog?.kind === "filters" && <FiltersDialog source={dialog.name} onClose={() => setDialog(null)} />}
    </Panel>
  );
}
