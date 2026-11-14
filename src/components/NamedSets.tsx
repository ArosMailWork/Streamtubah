// Picker for named sets stored as files: scene collections and profiles.
import { useEffect, useState } from "react";
import { ask } from "@tauri-apps/plugin-dialog";
import { act } from "../store";
import { PromptDialog } from "./Modal";

export interface NamedSetOps {
  list: () => Promise<{ current: string; list: string[] }>;
  switchTo: (name: string) => Promise<void>;
  create: (name: string) => Promise<void>;
  duplicate?: (name: string) => Promise<void>;
  rename: (name: string, newName: string) => Promise<void>;
  remove: (name: string) => Promise<void>;
}

type Dialog = null | "new" | "duplicate" | "rename";

export function NamedSetPicker(props: { label: string; ops: NamedSetOps; disabled?: boolean; onChanged?: () => void }) {
  const [sets, setSets] = useState<{ current: string; list: string[] }>({ current: "", list: [] });
  const [dialog, setDialog] = useState<Dialog>(null);
  const reload = () => props.ops.list().then(setSets);
  useEffect(() => { reload(); }, []);

  const run = async (fn: () => Promise<void>) => {
    await act(fn);
    await reload();
    props.onChanged?.();
  };
  const cur = sets.current;

  return (
    <div className="named-set">
      <span className="prop-label">{props.label}</span>
      <select value={cur} disabled={props.disabled} onChange={(e) => run(() => props.ops.switchTo(e.target.value))}>
        {sets.list.map((n) => <option key={n} value={n}>{n}</option>)}
      </select>
      <select className="named-set-menu" value="" disabled={props.disabled}
        onChange={(e) => {
          const v = e.target.value;
          if (v === "delete") {
            ask(`Delete "${cur}"? This cannot be undone.`, { kind: "warning" })
              .then((yes) => { if (yes) run(() => props.ops.remove(cur)); });
          } else if (v) setDialog(v as Dialog);
        }}>
        <option value="">⋯</option>
        <option value="new">New…</option>
        {props.ops.duplicate && <option value="duplicate">Duplicate…</option>}
        <option value="rename">Rename…</option>
        <option value="delete" disabled={sets.list.length < 2}>Delete</option>
      </select>
      {dialog === "new" && (
        <PromptDialog title={`New ${props.label}`} label="Name" confirm="Create"
          onSubmit={(n) => run(() => props.ops.create(n))} onClose={() => setDialog(null)} />
      )}
      {dialog === "duplicate" && props.ops.duplicate && (
        <PromptDialog title={`Duplicate ${props.label}`} label="Name" initial={`${cur} copy`} confirm="Duplicate"
          onSubmit={(n) => run(() => props.ops.duplicate!(n))} onClose={() => setDialog(null)} />
      )}
      {dialog === "rename" && (
        <PromptDialog title={`Rename ${props.label}`} label="Name" initial={cur} confirm="Rename"
          onSubmit={(n) => run(() => props.ops.rename(cur, n))} onClose={() => setDialog(null)} />
      )}
    </div>
  );
}
