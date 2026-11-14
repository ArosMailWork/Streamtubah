// Edit Transform and per-item Show/Hide transition dialogs.
import { useEffect, useState } from "react";
import { api, ItemTransform, TypeInfo } from "../api";
import { act } from "../store";
import { Modal } from "./Modal";

// libobs alignment flags: 0 = center, 1 left, 2 right, 4 top, 8 bottom.
const ALIGNMENTS: [number, string][] = [
  [5, "Top Left"], [4, "Top"], [6, "Top Right"],
  [1, "Left"], [0, "Center"], [2, "Right"],
  [9, "Bottom Left"], [8, "Bottom"], [10, "Bottom Right"],
];
const BOUNDS: [number, string][] = [
  [0, "No bounds"], [1, "Stretch to bounds"], [2, "Scale to inner bounds (fit)"], [3, "Scale to outer bounds (fill)"],
  [4, "Scale to width of bounds"], [5, "Scale to height of bounds"], [6, "Maximum size only"],
];

function Num(props: { label: string; value: number; step?: number; onChange: (v: number) => void; suffix?: string }) {
  const [text, setText] = useState(String(props.value));
  useEffect(() => setText(String(Math.round(props.value * 100) / 100)), [props.value]);
  const commit = () => {
    const v = Number(text);
    if (Number.isFinite(v)) props.onChange(v);
    else setText(String(props.value));
  };
  return (
    <label className="num-field">
      <span className="muted small">{props.label}</span>
      <input type="number" step={props.step ?? 1} value={text} onChange={(e) => setText(e.target.value)}
        onBlur={commit} onKeyDown={(e) => e.key === "Enter" && commit()} />
      {props.suffix && <span className="muted small">{props.suffix}</span>}
    </label>
  );
}

export function EditTransformDialog(props: { id: number; name: string; onClose: () => void }) {
  const [t, setT] = useState<ItemTransform | null>(null);
  useEffect(() => { api.itemTransformGet(props.id).then(setT); }, [props.id]);
  if (!t) return null;

  // Changes apply immediately, like OBS.
  const apply = (next: ItemTransform) => {
    setT(next);
    act(() => api.itemTransformSet(props.id, next));
  };
  const [cl, ct, cr, cb] = t.crop;
  const visW = Math.max(1, t.source_size[0] - cl - cr);
  const visH = Math.max(1, t.source_size[1] - ct - cb);
  const width = Math.abs(t.scale[0]) * visW;
  const height = Math.abs(t.scale[1]) * visH;
  const withSign = (old: number, mag: number) => (old < 0 ? -mag : mag);

  return (
    <Modal title={`Edit Transform — ${props.name}`} onClose={props.onClose} wide
      footer={<>
        <button onClick={() => act(() => api.itemTransform(props.id, "reset")).then(() => api.itemTransformGet(props.id).then(setT))}>Reset</button>
        <button className="primary" onClick={props.onClose}>Done</button>
      </>}>
      <div className="transform-grid">
        <span className="prop-label">Position</span>
        <div className="row">
          <Num label="X" value={t.pos[0]} onChange={(v) => apply({ ...t, pos: [v, t.pos[1]] })} suffix="px" />
          <Num label="Y" value={t.pos[1]} onChange={(v) => apply({ ...t, pos: [t.pos[0], v] })} suffix="px" />
        </div>
        <span className="prop-label">Rotation</span>
        <div className="row"><Num label="" value={t.rot} step={0.5} onChange={(v) => apply({ ...t, rot: v })} suffix="°" /></div>
        <span className="prop-label">Size</span>
        <div className="row">
          <Num label="W" value={width} onChange={(v) => apply({ ...t, scale: [withSign(t.scale[0], Math.max(1, v) / visW), t.scale[1]] })} suffix="px" />
          <Num label="H" value={height} onChange={(v) => apply({ ...t, scale: [t.scale[0], withSign(t.scale[1], Math.max(1, v) / visH)] })} suffix="px" />
          <span className="muted small">source {t.source_size[0]}×{t.source_size[1]}</span>
        </div>
        <span className="prop-label">Positional alignment</span>
        <select value={t.alignment} onChange={(e) => apply({ ...t, alignment: +e.target.value })}>
          {ALIGNMENTS.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
        </select>
        <span className="prop-label">Bounding box</span>
        <select value={t.bounds_type} onChange={(e) => {
          const bt = +e.target.value;
          // Starting bounds from the current size keeps the item where it is.
          apply({ ...t, bounds_type: bt, bounds: t.bounds_type === 0 ? [width, height] : t.bounds });
        }}>
          {BOUNDS.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
        </select>
        {t.bounds_type !== 0 && (
          <>
            <span className="prop-label">Bounding box size</span>
            <div className="row">
              <Num label="W" value={t.bounds[0]} onChange={(v) => apply({ ...t, bounds: [Math.max(1, v), t.bounds[1]] })} suffix="px" />
              <Num label="H" value={t.bounds[1]} onChange={(v) => apply({ ...t, bounds: [t.bounds[0], Math.max(1, v)] })} suffix="px" />
            </div>
            <span className="prop-label">Alignment in bounding box</span>
            <select value={t.bounds_alignment} onChange={(e) => apply({ ...t, bounds_alignment: +e.target.value })}>
              {ALIGNMENTS.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
            </select>
          </>
        )}
        <span className="prop-label">Crop</span>
        <div className="row">
          {(["Left", "Top", "Right", "Bottom"] as const).map((l, i) => (
            <Num key={l} label={l} value={t.crop[i]} onChange={(v) => {
              const crop = [...t.crop] as ItemTransform["crop"];
              crop[i] = Math.max(0, Math.round(v));
              apply({ ...t, crop });
            }} />
          ))}
        </div>
      </div>
    </Modal>
  );
}

export function ItemTransitionDialog(props: { id: number; name: string; onClose: () => void }) {
  const [types, setTypes] = useState<TypeInfo[]>([]);
  const [cfg, setCfg] = useState<{ show: { type_id: string; duration_ms: number }; hide: { type_id: string; duration_ms: number } } | null>(null);
  useEffect(() => {
    api.transitionTypes().then(setTypes);
    api.itemTransitions(props.id).then(setCfg);
  }, [props.id]);
  if (!cfg) return null;

  const set = (which: "show" | "hide", type_id: string, duration_ms: number) => {
    setCfg({ ...cfg, [which]: { type_id, duration_ms } });
    act(() => api.itemTransitionSet(props.id, which === "show", type_id, duration_ms));
  };
  const row = (which: "show" | "hide", label: string) => (
    <div className="prop-row">
      <span className="prop-label">{label}</span>
      <div className="row">
        <select className="grow" value={cfg[which].type_id} onChange={(e) => set(which, e.target.value, cfg[which].duration_ms || 300)}>
          <option value="">None (instant)</option>
          {types.filter((t) => t.id !== "obs_stinger_transition").map((t) => <option key={t.id} value={t.id}>{t.name}</option>)}
        </select>
        <Num label="" value={cfg[which].duration_ms || 300} onChange={(v) => set(which, cfg[which].type_id, Math.round(v))} suffix="ms" />
      </div>
    </div>
  );
  return (
    <Modal title={`Show / Hide Transition — ${props.name}`} onClose={props.onClose}
      footer={<button className="primary" onClick={props.onClose}>Done</button>}>
      <div className="prop-info">Plays when you click the eye icon (or a hotkey/remote toggles the source).</div>
      {row("show", "When shown")}
      {row("hide", "When hidden")}
    </Modal>
  );
}
