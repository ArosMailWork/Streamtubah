// Layout gallery that slides in from the left (like Unity's layout menu, but
// visual): pick a preset, save the current arrangement, manage your presets.
// Arranging panels happens in the workspace by dragging their headers.
import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { act, holdDisplaysHidden } from "../store";
import {
  deletePreset, duplicatePreset, isModified, LayoutNode, Mode, MODES, Preset, resetPreset, savePresetAs,
  setActive, updatePreset, useLayouts, widgetInfo, WidgetId,
} from "./model";

interface Cell {
  widget: WidgetId;
  x: number;
  y: number;
  w: number;
  h: number;
}

/** Flattens a split tree into absolute rectangles (percent of the thumbnail). */
function cells(n: LayoutNode, x = 0, y = 0, w = 100, h = 100): Cell[] {
  if (n.type === "leaf") return [{ widget: n.widget, x, y, w, h }];
  const total = n.sizes.reduce((a, b) => a + b, 0) || 1;
  let offset = 0;
  return n.children.flatMap((c, i) => {
    const part = (n.sizes[i] ?? 0) / total;
    const r = n.dir === "row" ? cells(c, x + w * offset, y, w * part, h) : cells(c, x, y + h * offset, w, h * part);
    offset += part;
    return r;
  });
}

export function LayoutThumb({ root, chat }: { root: LayoutNode; chat: boolean }) {
  return (
    <div className="layout-thumb">
      <div className="layout-thumb-canvas" style={{ right: chat ? "22%" : 0 }}>
        {cells(root).map((c) => {
          const info = widgetInfo(c.widget);
          return (
            <div key={c.widget} className={`thumb-cell ${c.widget}`} title={info.name}
              style={{ left: `${c.x}%`, top: `${c.y}%`, width: `${c.w}%`, height: `${c.h}%` }}>
              <span className="thumb-icon">{info.icon}</span>
              {/* Small cells show only the icon so labels never get clipped. */}
              {c.w >= 14 && c.h >= 17 && <span className="thumb-name">{info.name}</span>}
            </div>
          );
        })}
      </div>
      {chat && (
        <div className="thumb-cell chat thumb-chat">
          <span className="thumb-icon">💬</span>
          <span className="thumb-name">Chat</span>
        </div>
      )}
    </div>
  );
}

/** Applies a preset's mode and chat state to the app. */
export async function applyPreset(p: Preset, setChatOpen: (open: boolean) => void) {
  setActive(p.id);
  setChatOpen(p.chatOpen);
  await act(() => api.studioModeSet(p.mode === "studio"));
}

function SaveForm(props: { onDone: () => void; setChatOpen: (o: boolean) => void; current: Preset }) {
  const [name, setName] = useState(`${props.current.name} (custom)`);
  const [mode, setMode] = useState<Mode>(props.current.mode);
  const [chat, setChat] = useState(props.current.chatOpen);
  const save = () => {
    if (!name.trim()) return;
    savePresetAs(name.trim(), mode, chat);
    props.setChatOpen(chat);
    act(() => api.studioModeSet(mode === "studio"));
    props.onDone();
  };
  return (
    <div className="save-form">
      <input autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="Layout name"
        onKeyDown={(e) => { if (e.key === "Enter") save(); if (e.key === "Escape") props.onDone(); }} />
      <select value={mode} onChange={(e) => setMode(e.target.value as Mode)}>
        {MODES.map((m) => <option key={m.id} value={m.id}>{m.name} mode</option>)}
      </select>
      <label className="prop-check"><input type="checkbox" checked={chat} onChange={(e) => setChat(e.target.checked)} /> Open chat</label>
      <button onClick={props.onDone}>Cancel</button>
      <button className="primary" disabled={!name.trim()} onClick={save}>Save layout</button>
    </div>
  );
}

function PresetCard(props: { p: Preset; active: boolean; onPick: () => void }) {
  const { p } = props;
  const [renaming, setRenaming] = useState(false);
  const modified = isModified(p);
  return (
    <div className={`layout-card ${props.active ? "active" : ""}`}>
      <button className="layout-card-main" onClick={props.onPick}>
        <LayoutThumb root={p.root} chat={p.chatOpen} />
      </button>
      <div className="layout-card-footer">
        {renaming ? (
          <input autoFocus className="grow" defaultValue={p.name}
            onBlur={(e) => { updatePreset(p.id, { name: e.target.value.trim() || p.name }); setRenaming(false); }}
            onKeyDown={(e) => { if (e.key === "Enter" || e.key === "Escape") (e.target as HTMLInputElement).blur(); }} />
        ) : (
          <div className="layout-card-title grow" onDoubleClick={() => !p.builtin && setRenaming(true)}>
            <b>{p.name}</b>
            <div className="layout-card-badges">
              <span className={`badge mode-${p.mode}`}>{MODES.find((m) => m.id === p.mode)!.name}</span>
              {p.chatOpen && <span className="badge">Chat</span>}
              {!p.builtin && <span className="badge">Custom</span>}
              {modified && <span className="badge">Edited</span>}
              {props.active && <span className="badge active">Active</span>}
            </div>
          </div>
        )}
        <div className="layout-card-actions">
          <button className="icon small" title="Duplicate" onClick={() => duplicatePreset(p.id)}>⧉</button>
          {!p.builtin && <button className="icon small" title="Rename" onClick={() => setRenaming(true)}>✎</button>}
          {p.builtin
            ? <button className="icon small" title="Reset to default" disabled={!modified} onClick={() => resetPreset(p.id)}>↺</button>
            : <button className="icon small danger" title="Delete" onClick={() => deletePreset(p.id)}>🗑</button>}
        </div>
      </div>
    </div>
  );
}

export function LayoutDrawer(props: { setChatOpen: (open: boolean) => void }) {
  const { presets, active } = useLayouts();
  const [open, setOpen] = useState(false);
  const [filter, setFilter] = useState<Mode | "all">("all");
  const [saving, setSaving] = useState(false);
  const release = useRef<(() => void) | null>(null);
  const drawer = useRef<HTMLDivElement>(null);

  const toggle = () => {
    if (!open) {
      release.current?.();
      release.current = holdDisplaysHidden();
    }
    setOpen(!open);
  };

  // Show displays again only after the closing slide has finished.
  useEffect(() => {
    const el = drawer.current;
    if (!el) return;
    const done = (e: TransitionEvent) => {
      if (e.target === el && !open) {
        release.current?.();
        release.current = null;
      }
    };
    el.addEventListener("transitionend", done);
    return () => el.removeEventListener("transitionend", done);
  }, [open]);
  useEffect(() => () => release.current?.(), []);

  const current = presets.find((p) => p.id === active) ?? presets[0];
  const groups = MODES.filter((m) => filter === "all" || m.id === filter);

  return (
    <div ref={drawer} className={`layout-drawer ${open ? "open" : ""}`}>
      <button className="edge-tab left-tab" onClick={toggle} title={open ? "Close layouts" : "Layouts & modes"}>
        <span className="edge-arrow">{open ? "‹" : "›"}</span>
        <span className="edge-label">LAYOUT</span>
      </button>

      <div className="layout-drawer-body">
        <div className="layout-gallery">
          <header className="gallery-header">
            <div className="grow">
              <h1>Layouts</h1>
              <p className="muted">
                Pick a layout. To make your own, drag panels by their headers in the workspace (center to swap,
                edges to split), resize the borders, then save it here.
              </p>
            </div>
            {!saving && <button className="primary big" onClick={() => setSaving(true)}>＋ Save current layout</button>}
          </header>
          {saving && <SaveForm current={current} setChatOpen={props.setChatOpen} onDone={() => setSaving(false)} />}

          <div className="mode-filter">
            {[{ id: "all" as const, name: "All" }, ...MODES].map((m) => (
              <button key={m.id} className={filter === m.id ? "active" : ""} onClick={() => setFilter(m.id)}>{m.name}</button>
            ))}
          </div>

          {groups.map((m) => {
            const list = presets.filter((p) => p.mode === m.id);
            if (list.length === 0) return null;
            return (
              <section key={m.id} className="gallery-section">
                <h2>{m.name} <span className="muted">— {m.hint}</span></h2>
                <div className="layout-grid">
                  {list.map((p) => (
                    <PresetCard key={p.id} p={p} active={p.id === active} onPick={() => applyPreset(p, props.setChatOpen)} />
                  ))}
                </div>
              </section>
            );
          })}
        </div>
      </div>
    </div>
  );
}
