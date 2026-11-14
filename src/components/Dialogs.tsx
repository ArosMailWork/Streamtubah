// Source-related dialogs: add source, properties, filters.
import { useEffect, useMemo, useState } from "react";
import { api, FilterInfo, PropTarget, TypeInfo } from "../api";
import { act } from "../store";
import { Modal } from "./Modal";
import { PropertiesForm } from "./PropertiesForm";
import { CATEGORIES, Category, featuredRank, sourceDoc } from "./sourceCatalog";

export function AddSourceDialog(props: { onClose: () => void; onCreated: (name: string) => void }) {
  const [types, setTypes] = useState<TypeInfo[]>([]);
  const [existing, setExisting] = useState<string[]>([]);
  const [category, setCategory] = useState<Category | "All" | "Existing">("All");
  const [query, setQuery] = useState("");
  const [picked, setPicked] = useState<TypeInfo | null>(null);
  const [name, setName] = useState("");

  useEffect(() => {
    api.sourceTypes().then((t) => {
      setTypes(t);
      const first = [...t].sort((a, b) => featuredRank(a.id) - featuredRank(b.id))[0];
      if (first) { setPicked(first); setName(first.name); }
    });
    api.existingSources().then(setExisting);
  }, []);

  const docOf = (t: TypeInfo) => sourceDoc(t.id, t.video, t.audio);
  const visible = useMemo(() => {
    const q = query.trim().toLowerCase();
    return types
      .filter((t) => category === "All" || docOf(t).category === category)
      .filter((t) => !q || t.name.toLowerCase().includes(q) || docOf(t).summary.toLowerCase().includes(q))
      .sort((a, b) => featuredRank(a.id) - featuredRank(b.id) || a.name.localeCompare(b.name));
  }, [types, category, query]);

  const pick = (t: TypeInfo) => { setPicked(t); setName(t.name); };
  const create = async () => {
    if (!picked) return;
    const created = await act(() => api.sourceCreate(picked.id, name.trim() || picked.name));
    if (created) {
      props.onClose();
      props.onCreated(created);
    }
  };
  const doc = picked && docOf(picked);
  const counts = (c: Category) => types.filter((t) => docOf(t).category === c).length;

  return (
    <Modal title="Add Source" onClose={props.onClose} wide
      footer={category !== "Existing" && <>
        <button onClick={props.onClose}>Cancel</button>
        <button className="primary" disabled={!picked} onClick={create}>Add Source</button>
      </>}>
      <div className="add-source">
        <nav className="add-source-cats">
          <input placeholder="Search sources…" value={query} onChange={(e) => { setQuery(e.target.value); if (category === "Existing") setCategory("All"); }} />
          <button className={category === "All" ? "active" : ""} onClick={() => setCategory("All")}>All</button>
          {CATEGORIES.filter((c) => counts(c) > 0).map((c) => (
            <button key={c} className={category === c ? "active" : ""} onClick={() => setCategory(c)}>{c} <span className="muted">{counts(c)}</span></button>
          ))}
          {existing.length > 0 && (
            <button className={category === "Existing" ? "active" : ""} onClick={() => setCategory("Existing")}>Existing sources <span className="muted">{existing.length}</span></button>
          )}
        </nav>

        {category === "Existing" ? (
          <div className="add-source-existing">
            <div className="prop-info">Adds the same source again: changes to it show up everywhere it is used.</div>
            <div className="list">
              {existing.map((n) => (
                <div key={n} className="list-row" onClick={() => act(() => api.sourceAddExisting(n)).then(props.onClose)}>
                  <span className="grow">{n}</span><span className="muted small">Add</span>
                </div>
              ))}
            </div>
          </div>
        ) : (
          <>
            <div className="add-source-list list">
              {visible.map((t) => (
                <div key={t.id} className={`list-row ${picked?.id === t.id ? "active" : ""}`} onClick={() => pick(t)} onDoubleClick={create}>
                  <span className="type-icon">{docOf(t).icon}</span>
                  <span className="grow">{t.name}</span>
                </div>
              ))}
              {visible.length === 0 && <div className="muted small">No source type matches.</div>}
            </div>
            <div className="add-source-doc">
              {picked && doc && (
                <>
                  <h3><span className="type-icon">{doc.icon}</span> {picked.name}</h3>
                  <p>{doc.summary}</p>
                  {doc.tips.length > 0 && <ul className="tips">{doc.tips.map((t) => <li key={t}>{t}</li>)}</ul>}
                  <label className="field">
                    <span>Name</span>
                    <input value={name} onChange={(e) => setName(e.target.value)} onKeyDown={(e) => e.key === "Enter" && create()} />
                  </label>
                </>
              )}
            </div>
          </>
        )}
      </div>
    </Modal>
  );
}

export function PropertiesDialog(props: { title: string; target: PropTarget; onClose: () => void }) {
  return (
    <Modal title={props.title} onClose={props.onClose} wide footer={<button className="primary" onClick={props.onClose}>Done</button>}>
      <PropertiesForm target={props.target} />
    </Modal>
  );
}

export function FiltersDialog(props: { source: string; onClose: () => void }) {
  const [filters, setFilters] = useState<FilterInfo[]>([]);
  const [types, setTypes] = useState<TypeInfo[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [addType, setAddType] = useState("");

  const reload = async () => {
    const list = await api.filters(props.source);
    setFilters(list);
    setSelected((cur) => (cur && list.some((f) => f.name === cur) ? cur : list[0]?.name ?? null));
  };
  useEffect(() => {
    reload();
    api.filterTypes(props.source).then((t) => {
      setTypes(t);
      setAddType(t[0]?.id ?? "");
    });
  }, [props.source]);

  const run = async (fn: () => Promise<unknown>) => {
    await act(fn);
    await reload();
  };

  const add = async () => {
    const t = types.find((x) => x.id === addType);
    if (!t) return;
    const name = await act(() => api.filterAdd(props.source, t.id, t.name));
    await reload();
    if (name) setSelected(name);
  };

  return (
    <Modal title={`Filters — ${props.source}`} onClose={props.onClose} wide footer={<button className="primary" onClick={props.onClose}>Done</button>}>
      <div className="split">
        <div className="split-list">
          <div className="list">
            {filters.map((f) => (
              <div key={f.name} className={`list-row ${selected === f.name ? "active" : ""}`} onClick={() => setSelected(f.name)}>
                <input type="checkbox" checked={f.enabled} onClick={(e) => e.stopPropagation()}
                  onChange={(e) => run(() => api.filterSetEnabled(props.source, f.name, e.target.checked))} />
                <span className="grow">{f.name}</span>
                <span className="muted small">{f.type_name}</span>
              </div>
            ))}
            {filters.length === 0 && <div className="empty">No filters yet</div>}
          </div>
          <div className="row">
            <select value={addType} onChange={(e) => setAddType(e.target.value)} className="grow">
              {types.map((t) => <option key={t.id} value={t.id}>{t.name}</option>)}
            </select>
            <button onClick={add} disabled={!addType}>Add</button>
          </div>
          {selected && (
            <div className="row">
              <button onClick={() => run(() => api.filterMove(props.source, selected, true))}>▲</button>
              <button onClick={() => run(() => api.filterMove(props.source, selected, false))}>▼</button>
              <button className="danger" onClick={() => run(() => api.filterRemove(props.source, selected))}>Remove</button>
            </div>
          )}
        </div>
        <div className="split-detail">
          {selected ? (
            <PropertiesForm target={{ kind: "filter", source: props.source, filter: selected }} />
          ) : (
            <div className="muted">Add a filter (noise suppression, chroma key, color correction…) to get started.</div>
          )}
        </div>
      </div>
    </Modal>
  );
}
