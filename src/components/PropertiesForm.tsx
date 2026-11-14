// Generic renderer for libobs obs_properties_t descriptions.
import { useEffect, useRef, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { api, errorText, PropertiesView, Property, PropTarget } from "../api";
import { toast } from "../store";

type Settings = Record<string, unknown>;

/** OBS stores colors as 0xAABBGGRR. */
function obsColorToHex(v: unknown): string {
  const n = Number(v ?? 0) >>> 0;
  const r = n & 0xff, g = (n >> 8) & 0xff, b = (n >> 16) & 0xff;
  return "#" + [r, g, b].map((x) => x.toString(16).padStart(2, "0")).join("");
}
function hexToObsColor(hex: string, prev: unknown): number {
  const alpha = ((Number(prev ?? 0xff000000) >>> 24) & 0xff) || 0xff;
  const r = parseInt(hex.slice(1, 3), 16), g = parseInt(hex.slice(3, 5), 16), b = parseInt(hex.slice(5, 7), 16);
  return ((alpha << 24) | (b << 16) | (g << 8) | r) >>> 0;
}

/** "Images (*.png *.jpg);;All Files (*.*)" -> dialog filters */
function parseFilter(f?: string) {
  if (!f) return undefined;
  const out = f
    .split(";;")
    .map((part) => {
      const m = part.match(/^(.*?)\s*\(([^)]*)\)/);
      if (!m) return null;
      const exts = m[2].split(/\s+/).map((e) => e.replace(/^\*\.?/, "")).filter((e) => e && e !== "*");
      return exts.length ? { name: m[1] || "Files", extensions: exts } : null;
    })
    .filter(Boolean) as { name: string; extensions: string[] }[];
  return out.length ? out : undefined;
}

/** Text input that commits on blur / Enter / short idle, to avoid a backend round-trip per keystroke. */
function LazyText(props: {
  value: string;
  onCommit: (v: string) => void;
  type?: string;
  multiline?: boolean;
  disabled?: boolean;
  list?: string;
}) {
  const [v, setV] = useState(props.value);
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => setV(props.value), [props.value]);
  const commit = (x: string) => {
    window.clearTimeout(timer.current);
    if (x !== props.value) props.onCommit(x);
  };
  const change = (x: string) => {
    setV(x);
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => commit(x), 600);
  };
  return props.multiline ? (
    <textarea value={v} disabled={props.disabled} rows={5} onChange={(e) => change(e.target.value)} onBlur={() => commit(v)} />
  ) : (
    <input
      type={props.type ?? "text"}
      value={v}
      disabled={props.disabled}
      list={props.list}
      onChange={(e) => change(e.target.value)}
      onBlur={() => commit(v)}
      onKeyDown={(e) => e.key === "Enter" && commit(v)}
    />
  );
}

function Field(props: { p: Property; s: Settings; set: (patch: Settings) => void; button: (name: string) => void }) {
  const { p, s, set } = props;
  const value = s[p.name];
  const disabled = !p.enabled;

  switch (p.type) {
    case "bool":
      return (
        <label className="prop-check">
          <input type="checkbox" checked={!!value} disabled={disabled} onChange={(e) => set({ [p.name]: e.target.checked })} />
          {p.description}
        </label>
      );

    case "int":
    case "float": {
      const num = Number(value ?? 0);
      const parse = (x: string) => (p.type === "int" ? parseInt(x, 10) : parseFloat(x));
      return (
        <Row p={p}>
          <div className="prop-number">
            {p.slider && (
              <input type="range" min={p.min} max={p.max} step={p.step} value={num} disabled={disabled}
                onChange={(e) => set({ [p.name]: parse(e.target.value) })} />
            )}
            <LazyText type="number" value={String(num)} disabled={disabled}
              onCommit={(x) => !Number.isNaN(parse(x)) && set({ [p.name]: Math.min(p.max ?? Infinity, Math.max(p.min ?? -Infinity, parse(x))) })} />
            {p.suffix && <span className="muted">{p.suffix}</span>}
          </div>
        </Row>
      );
    }

    case "text":
      if (p.text_type === "info") {
        return <div className={`prop-info ${p.info_type}`}>{p.description}{value ? `: ${value}` : ""}</div>;
      }
      return (
        <Row p={p}>
          <LazyText value={String(value ?? "")} disabled={disabled} multiline={p.text_type === "multiline"}
            type={p.text_type === "password" ? "password" : "text"} onCommit={(x) => set({ [p.name]: x })} />
        </Row>
      );

    case "path": {
      const browse = async () => {
        const filters = parseFilter(p.filter);
        const defaultPath = String(value || p.default_path || "") || undefined;
        const picked =
          p.path_type === "file_save"
            ? await save({ filters, defaultPath })
            : await open({ directory: p.path_type === "directory", filters, defaultPath });
        if (typeof picked === "string") set({ [p.name]: picked });
      };
      return (
        <Row p={p}>
          <div className="prop-path">
            <LazyText value={String(value ?? "")} disabled={disabled} onCommit={(x) => set({ [p.name]: x })} />
            <button disabled={disabled} onClick={browse}>Browse</button>
          </div>
        </Row>
      );
    }

    case "list": {
      const items = p.items ?? [];
      const coerce = (raw: string) => {
        const it = items.find((i) => String(i.value) === raw);
        if (it) return it.value;
        return p.format === "int" ? parseInt(raw, 10) : p.format === "float" ? parseFloat(raw) : raw;
      };
      if (p.editable) {
        const id = `dl-${p.name}`;
        return (
          <Row p={p}>
            <LazyText value={String(value ?? "")} list={id} disabled={disabled} onCommit={(x) => set({ [p.name]: coerce(x) })} />
            <datalist id={id}>{items.map((i) => <option key={String(i.value)} value={String(i.value)}>{i.name}</option>)}</datalist>
          </Row>
        );
      }
      const known = items.some((i) => String(i.value) === String(value));
      return (
        <Row p={p}>
          <select value={String(value ?? "")} disabled={disabled} onChange={(e) => set({ [p.name]: coerce(e.target.value) })}>
            {!known && <option value={String(value ?? "")}>{value ? String(value) : "—"}</option>}
            {items.map((i) => (
              <option key={String(i.value)} value={String(i.value)} disabled={i.disabled}>{i.name}</option>
            ))}
          </select>
        </Row>
      );
    }

    case "color":
    case "color_alpha":
      return (
        <Row p={p}>
          <input type="color" value={obsColorToHex(value)} disabled={disabled}
            onChange={(e) => set({ [p.name]: hexToObsColor(e.target.value, value) })} />
        </Row>
      );

    case "button":
      return (
        <div className="prop-row">
          <span />
          <button disabled={disabled} onClick={() => props.button(p.name)}>{p.description}</button>
        </div>
      );

    case "font": {
      const font = (value ?? {}) as { face?: string; size?: number; style?: string; flags?: number };
      return (
        <Row p={p}>
          <div className="prop-font">
            <LazyText value={font.face ?? ""} disabled={disabled} onCommit={(face) => set({ [p.name]: { ...font, face } })} />
            <LazyText type="number" value={String(font.size ?? 32)} disabled={disabled}
              onCommit={(x) => set({ [p.name]: { ...font, size: parseInt(x, 10) || 32 } })} />
          </div>
        </Row>
      );
    }

    case "editable_list": {
      const list = (Array.isArray(value) ? value : []) as { value: string; selected?: boolean; hidden?: boolean }[];
      const update = (next: typeof list) => set({ [p.name]: next });
      const add = async () => {
        if (p.list_type === "strings") {
          update([...list, { value: "", selected: false, hidden: false }]);
          return;
        }
        const picked = await open({ multiple: true, filters: parseFilter(p.filter) });
        const files = Array.isArray(picked) ? picked : picked ? [picked] : [];
        update([...list, ...files.map((f) => ({ value: f, selected: false, hidden: false }))]);
      };
      return (
        <Row p={p}>
          <div className="prop-elist">
            {list.map((it, i) => (
              <div key={i} className="prop-elist-row">
                <LazyText value={it.value} onCommit={(x) => update(list.map((o, j) => (j === i ? { ...o, value: x } : o)))} />
                <button onClick={() => update(list.filter((_, j) => j !== i))}>✕</button>
              </div>
            ))}
            <button disabled={disabled} onClick={add}>+ Add</button>
          </div>
        </Row>
      );
    }

    case "group":
      return (
        <fieldset className="prop-group">
          <legend>
            {p.checkable ? (
              <label className="prop-check">
                <input type="checkbox" checked={!!value} onChange={(e) => set({ [p.name]: e.target.checked })} />
                {p.description}
              </label>
            ) : (
              p.description
            )}
          </legend>
          {(!p.checkable || !!value) && <Fields list={p.children ?? []} s={s} set={set} button={props.button} />}
        </fieldset>
      );

    default:
      return <Row p={p}><span className="muted">Not editable here</span></Row>;
  }
}

function Row(props: { p: Property; children: React.ReactNode }) {
  return (
    <div className="prop-row" title={props.p.long_description || undefined}>
      <span className="prop-label">{props.p.description}</span>
      <div className="prop-control">{props.children}</div>
    </div>
  );
}

function Fields(props: { list: Property[]; s: Settings; set: (patch: Settings) => void; button: (n: string) => void }) {
  return (
    <>
      {props.list.filter((p) => p.visible).map((p) => (
        <Field key={p.name} p={p} s={props.s} set={props.set} button={props.button} />
      ))}
    </>
  );
}

/** Loads and edits the properties of one target; every change is applied live. */
export function PropertiesForm(props: { target: PropTarget }) {
  const [view, setView] = useState<PropertiesView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const key = JSON.stringify(props.target);

  useEffect(() => {
    setView(null);
    setError(null);
    api.properties(props.target).then(setView).catch((e) => setError(errorText(e)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  const run = (p: Promise<PropertiesView>) =>
    p.then(setView).catch((e) => toast(errorText(e), "error"));

  if (error) return <div className="muted">{error}</div>;
  if (!view) return <div className="muted">Loading…</div>;
  if (view.properties.length === 0) return <div className="muted">No settings for this item.</div>;

  return (
    <div className="props">
      <Fields
        list={view.properties}
        s={view.settings}
        set={(patch) => {
          setView({ ...view, settings: { ...view.settings, ...patch } });
          run(api.propertiesUpdate(props.target, patch));
        }}
        button={(name) => run(api.propertiesButton(props.target, name))}
      />
    </div>
  );
}
